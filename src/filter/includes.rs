use nom::{IResult, multispace, not_line_ending, space};
use local_encoding::{Encoder, Encoding};
use std::fmt;
use std::fmt::{Display, Formatter};
use std::io::{Error, ErrorKind};

#[derive(Debug)]
#[derive(PartialEq)]
pub enum Include<T> {
    Quote(T),
    Bracket(T),
}

impl<T> Display for Include<T>
    where T: Display
{
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            &Include::Quote(ref path) => write!(f, "\"{}\"", path),
            &Include::Bracket(ref path) => write!(f, "<{}>", path),
        }
    }
}

impl<T> Include<T> {
    pub fn map<O, F: Fn(T) -> O>(self, f: F) -> Include<O> {
        match self {
            Include::Quote(path) => Include::Quote(f(path)),
            Include::Bracket(path) => Include::Bracket(f(path)),
        }
    }
    pub fn map_or<O, E, F: Fn(T) -> Result<O, E>>(self, f: F) -> Result<Include<O>, E> {
        match self {
            Include::Quote(path) => f(path).map(|p| Include::Quote(p)),
            Include::Bracket(path) => f(path).map(|p| Include::Bracket(p)),
        }
    }
}

named!(eol, alt!(tag!(&b"\n"[..]) | tag!(&b"\r\n"[..])));

named!(comment_one_line,
       chain!(
           tag!(&b"//"[..]) ~
           not_line_ending? ,
           || {b""} ));

named!(comment_block,
       chain!(
           tag!(&b"/*"[..]) ~
           take_until_and_consume!(&b"*/"[..]),
           || {b""} ));

named!(blanks,
       chain!(many0!(alt!(space | comment_one_line | comment_block)),
              || b""));

named!(lines,
       chain!(many0!(alt!(multispace | comment_one_line | comment_block)),
              || b""));

named!(include_bracket<&[u8], Include<Vec<u8>>>,
        chain!(tag!(&b"<"[..]) ~
            include: take_until_either!("\r\n>")  ~
            tag!(&b">"[..]),
            || Include::Bracket(include.to_vec())));

named!(include_quote<&[u8], Include<Vec<u8>>>,
        chain!(tag!(&b"\""[..]) ~
            include: take_until_either!("\r\n\"") ~
            tag!(&b"\""[..]),
            || Include::Quote(include.to_vec())));

named!(include<&[u8], Include<Vec<u8>> >,
        chain!(
            tag!(&b"include"[..]) ~
            blanks? ~
            include: alt!(include_quote | include_bracket) ~
            blanks ? ~
            alt!(eof!()|eol),
           || include));

named!(unknown_directive,
        chain!(
            not!(tag!(&b"include"[..])) ~
            fold_many0!(
                chain!(
                    take_till!(is_code_special) ~
                    alt!(eof!() | code_double_quote | comment_one_line | comment_block | tag!(&b"/"[..])),
                    || b""
                ),
                &b""[..],
                |acc, _| acc
            ),
        || b""));

named!(directive<&[u8], Option<Include<Vec<u8>>> >,
        chain!(
            tag!(&b"#"[..]) ~
            blanks? ~
            include: alt!(
                map!(
                    include,
                    |v| Some(v)
                ) |
                map!(
                    unknown_directive,
                    |_| None
                )
            ),
            || include ));

fn is_code_special(c: u8) -> bool {
    match c {
        b'\r' | b'\n' | b'\"' | b'/' => true,
        _ => false,
    }
}

fn is_quote_special(c: u8) -> bool {
    match c {
        b'\r' | b'\n' | b'\"' | b'\\' => true,
        _ => false,
    }
}

named!(code_double_quote,
        chain!(
            tag!(&b"\""[..]) ~
            take_till!(is_quote_special) ~
            many0!(
                chain!(
                    tag!(&"\\"[..]) ~
                    take!(1) ~
                    take_till!(is_quote_special),
                    || {b""}
                )
            ) ~
            tag!(&b"\""[..]),
        || b""));

named!(code_line,
        chain!(
            not!(char!(b'#')) ~
            take_till!(is_code_special) ~
            many0!(
                chain!(
                    alt!(code_double_quote | comment_one_line | comment_block | tag!(&b"/"[..])) ~
                    take_till!(is_code_special),
                    || b""
                )
            ),
        || b""));

named!(find_includes<&[u8], (bool, Vec<Include<Vec<u8>>>)>,
  chain!(
    bom: tag!(&b"\xEF\xBB\xBF"[..]) ? ~
    lines ~
    includes: fold_many0!(
        chain!(
            include: alt!(
                directive |
                map!(
                    code_line,
                    |_| None
                )
            ) ~
            lines,
            || include
        ),
        Vec::new(),
        |mut acc: Vec<Include<Vec<u8>>>, include: Option<Include<Vec<u8>>>| {
            match include {
                 Some(v) => {acc.push(v); acc},
                 None => acc,
            }
        }
    ) ~
    eof!() ~
    move || {(bom.is_some(), includes)}
  )
);

pub fn source_includes(source: &[u8]) -> Result<Vec<Include<String>>, Error> {
    let result = match find_includes(source) {
        IResult::Incomplete(_) => {
            Err(Error::new(ErrorKind::UnexpectedEof,
                           "Unexpected end of stream. Can't parse source file"))
        }
        IResult::Error(_) => Err(Error::new(ErrorKind::InvalidData, "Can't parse source file")),
        IResult::Done(_, (bom, includes)) => {
            includes.into_iter()
                .map(|v| {
                    v.map_or(|path| match bom {
                        true => String::from_utf8(path).map_err(|e| Error::new(ErrorKind::InvalidData, e)),
                        false => Encoding::ANSI.to_string(&path),
                    })
                })
                .collect()
        }
    };
    result
}

#[test]
fn find_includes_simple() {
    let res = find_includes(b"\xEF\xBB\xBF#include <stdio.h>\n#include <stdlib.h>\n");
    println!("{:?}", res);
    assert_eq!(res, IResult::Done(&b""[..], (true, vec!(
        Include::Bracket(b"stdio.h".to_vec()),
        Include::Bracket(b"stdlib.h".to_vec())
    ))));
}

#[test]
fn code_line_test() {
    assert_eq!(code_line(b"void f() {}"), IResult::Done(&b""[..], &b""[..]));
    assert_eq!(code_line(b"void f() {}\n"), IResult::Done(&b"\n"[..], &b""[..]));
    assert_eq!(code_line(b"void f() //{}\n"), IResult::Done(&b"\n"[..], &b""[..]));
    assert_eq!(code_line(b"void f() {}\r"), IResult::Done(&b"\r"[..], &b""[..]));
    assert_eq!(code_line(b"char* f() /**\n* Bar\n*/ {return \"FOO\"}\n"), IResult::Done(&b"\n"[..], &b""[..]));
    assert_eq!(code_line(b"int f() {return 40 / 5;}\n"), IResult::Done(&b"\n"[..], &b""[..]));
}

#[test]
fn code_double_quote_test() {
    assert_eq!(code_double_quote(b"\"\""), IResult::Done(&b""[..], &b""[..]));
    assert_eq!(code_double_quote(b"\"Foo\""), IResult::Done(&b""[..], &b""[..]));
    assert_eq!(code_double_quote(b"\"Foo\\nBar\""), IResult::Done(&b""[..], &b""[..]));
    assert_eq!(code_double_quote(b"\"\\\"\""), IResult::Done(&b""[..], &b""[..]));
    assert!(code_double_quote(b"\"Foo\nBar\"").is_err());
    assert!(code_double_quote(b"\"Foo Bar").is_incomplete());
}

#[test]
fn find_includes_test() {
    let res = find_includes(br#"#include <iostream>
//#define FOO
#include <cstdlib> // For system
/* #include <stdafx.h> */
#include "stdio.h"
using namespace std;

int main()
{
    cout << "Hello, world!\n";
    cout << 10 / 2 /** Foo */;
    system("pause"); // MS Visual Studio
    return 0;
}"#);
    //    find_includes(b"\xEF\xBB\xBF#include <stdio.h>\n#include <stdlib.h>\n");

    println!("{:?}", res);
    assert_eq!(res, IResult::Done(&b""[..], (false, vec!(
        Include::Bracket ( b"iostream".to_vec()),
        Include::Bracket ( b"cstdlib".to_vec()),
        Include::Quote ( b"stdio.h".to_vec())
    ))));
}

#[test]
fn find_includes_cry1_test() {
    let r = find_includes(br#"////////////////////////////////////////////////////////////////////////////
#include DEVIRTUALIZE_HEADER_FIX(ITacticalPointSystem.h)

#ifndef __ITacticalPointSystem_h__
#define __ITacticalPointSystem_h__
#pragma once"#);
    println!("{:?}", r);
    assert!(!r.is_done());
}

#[test]
fn find_includes_cry2_test() {
    assert_eq!( find_includes(br#"////////////////////////////////////////////////////////////////////////////
#include "ITacticalPointSystem.h"

#ifndef __ITacticalPointSystem_h__
#define __ITacticalPointSystem_h__
#pragma once"#),IResult::Done(&b""[..], (false, vec!(        Include::Quote ( b"ITacticalPointSystem.h".to_vec())    ))));
}