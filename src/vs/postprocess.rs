use local_encoding::{Encoder, Encoding};

use libc;
use std::fmt::{Display, Formatter};
use std::io::{Error, ErrorKind, Read, Write};
use std::ptr;
use std::slice;
use std::ascii::AsciiExt;

#[derive(Clone, Copy, Debug)]
pub enum PostprocessError {
    LiteralEol,
    LiteralEof,
    LiteralTooLong,
    EscapeEof,
    MarkerNotFound,
    InvalidLiteral,
    TokenTooLong,
}

pub trait PostprocessWrite: Write {
    fn is_source_separator(&mut self, marker: &[u8]) -> Result<bool, Error>;
}

const BUF_SIZE: usize = 0x10000;

impl Display for PostprocessError {
    fn fmt(&self, f: &mut Formatter) -> Result<(), ::std::fmt::Error> {
        match self {
            &PostprocessError::LiteralEol => write!(f, "unexpected end of line in literal"),
            &PostprocessError::LiteralEof => write!(f, "unexpected end of stream in literal"),
            &PostprocessError::LiteralTooLong => write!(f, "literal too long"),
            &PostprocessError::EscapeEof => write!(f, "unexpected end of escape sequence"),
            &PostprocessError::MarkerNotFound => {
                write!(f,
                       "can't find precompiled header marker in preprocessed file")
            }
            &PostprocessError::InvalidLiteral => write!(f, "can't create string from literal"),
            &PostprocessError::TokenTooLong => write!(f, "token too long"),
        }
    }
}

impl ::std::error::Error for PostprocessError {
    fn description(&self) -> &str {
        match self {
            &PostprocessError::LiteralEol => "unexpected end of line in literal",
            &PostprocessError::LiteralEof => "unexpected end of stream in literal",
            &PostprocessError::LiteralTooLong => "literal too long",
            &PostprocessError::EscapeEof => "unexpected end of escape sequence",
            &PostprocessError::MarkerNotFound => "can't find precompiled header marker in preprocessed file",
            &PostprocessError::InvalidLiteral => "can't create string from literal",
            &PostprocessError::TokenTooLong => "token too long",
        }
    }

    fn cause(&self) -> Option<&::std::error::Error> {
        None
    }
}

pub fn filter_preprocessed(reader: &mut Read,
                           writer: &mut PostprocessWrite,
                           marker: &Option<String>,
                           keep_headers: bool)
                           -> Result<(), Error> {
    let mut state = ScannerState {
        buf_data: [0; BUF_SIZE],
        ptr_copy: ptr::null(),
        ptr_read: ptr::null(),
        ptr_end: ptr::null(),
        block: 0,

        reader: reader,
        writer: writer,

        keep_headers: keep_headers,
        marker: None,
        utf8: false,
        header_found: false,
        entry_file: None,
        done: false,
    };

    unsafe {
        state.ptr_copy = state.buf_data.as_ptr();
        state.ptr_read = state.buf_data.as_ptr();
        state.ptr_end = state.buf_data.as_ptr();

        try!(state.parse_bom());
        state.marker = match marker.as_ref() {
            Some(ref v) => {
                match state.utf8 {
                    true => Some(Vec::from(v.as_bytes())),
                    false => Some(try!(Encoding::ANSI.to_bytes(&v.replace("\\", "/")))),
                }
            }
            None => None,
        };
        loop {
            if state.ptr_read == state.ptr_end {
                if !try!(state.read()) {
                    break;
                }
            }
            try!(state.parse_line());
        }
        if state.done {
            Ok(())
        } else {
            Err(Error::new(ErrorKind::InvalidInput, PostprocessError::MarkerNotFound))
        }
    }
}

struct ScannerState<'a> {
    buf_data: [u8; BUF_SIZE],
    ptr_copy: *const u8,
    ptr_read: *const u8,
    ptr_end: *const u8,
    block: i32,

    reader: &'a mut Read,
    writer: &'a mut PostprocessWrite,

    keep_headers: bool,
    marker: Option<Vec<u8>>,

    utf8: bool,
    header_found: bool,
    entry_file: Option<Vec<u8>>,
    done: bool,
}

impl<'a> ScannerState<'a> {
    unsafe fn write(&mut self, data: &[u8]) -> Result<(), Error> {
        try!(self.flush());
        try!(self.writer.write(data));
        Ok(())
    }

    #[inline(always)]
    unsafe fn peek(&mut self) -> Result<Option<u8>, Error> {
        if self.ptr_read == self.ptr_end {
            if !try!(self.read()) {
                return Ok(None);
            }
        }
        Ok(Some(*self.ptr_read))
    }

    #[inline(always)]
    unsafe fn next(&mut self) {
        debug_assert!(self.ptr_read != self.ptr_end);
        self.ptr_read = self.ptr_read.offset(1);
    }

    unsafe fn preload(&mut self, need: usize) -> Result<(), Error> {
        try!(self.flush());

        self.block += 1;
        let mut loaded = delta(self.ptr_read, self.ptr_end);
        if loaded < need {
            let base = self.buf_data.as_mut_ptr();
            ptr::copy(self.ptr_read, base, loaded);
            while loaded < need {
                let read = try!(self.reader.read(&mut self.buf_data[loaded..]));
                if read == 0 {
                    break;
                }
                loaded += read;
            }
            self.ptr_read = base;
            self.ptr_copy = base;
            self.ptr_end = base.offset(loaded as isize);
        }
        Ok(())
    }
    unsafe fn read(&mut self) -> Result<bool, Error> {
        debug_assert!(self.ptr_read == self.ptr_end);
        try!(self.flush());
        let base = self.buf_data.as_ptr();
        self.block += 1;
        self.ptr_read = base;
        self.ptr_copy = base;
        self.ptr_end = base.offset(try!(self.reader.read(&mut self.buf_data)) as isize);
        Ok(self.ptr_read != self.ptr_end)
    }

    unsafe fn flush(&mut self) -> Result<(), Error> {
        if self.ptr_copy != self.ptr_read {
            if self.keep_headers || self.done {
                try!(self.writer.write(slice::from_raw_parts(self.ptr_copy, delta(self.ptr_copy, self.ptr_read))));
            }
            self.ptr_copy = self.ptr_read;
        }
        Ok(())
    }

    unsafe fn parse_bom(&mut self) -> Result<(), Error> {
        let bom: [u8; 3] = [0xEF, 0xBB, 0xBF];
        for bom_char in bom.iter() {
            match try!(self.peek()) {
                Some(c) if c == *bom_char => {
                    self.next();
                }
                Some(_) => {
                    return Ok(());
                }
                None => {
                    return Ok(());
                }
            };
        }
        self.utf8 = true;
        Ok(())
    }

    unsafe fn parse_line(&mut self) -> Result<(), Error> {
        try!(self.parse_empty());
        match try!(self.peek()) {
            Some(b'#') => self.parse_directive(),
            Some(_) => {
                try!(self.next_line());
                Ok(())
            }
            None => Ok(()),
        }
    }

    unsafe fn next_line(&mut self) -> Result<(), Error> {
        loop {
            let end = libc::memchr(self.ptr_read as *const libc::c_void,
                                   b'\n' as i32,
                                   delta(self.ptr_read, self.ptr_end)) as *const u8;
            if end != ptr::null() {
                self.ptr_read = end.offset(1);
                return Ok(());
            }
            self.ptr_read = self.ptr_end;
            if !try!(self.read()) {
                return Ok(());
            }
        }
    }

    unsafe fn next_line_eol(&mut self) -> Result<&'static [u8], Error> {
        let mut last: u8 = 0;
        loop {
            let end = libc::memchr(self.ptr_read as *const libc::c_void,
                                   b'\n' as i32,
                                   delta(self.ptr_read, self.ptr_end)) as *const u8;
            if end != ptr::null() {
                if end != &self.buf_data[0] {
                    last = *end.offset(-1);
                }
                self.ptr_read = end.offset(1);
                if last == b'\r' {
                    return Ok(b"\r\n");
                }
                return Ok(b"\n");
            }

            if self.ptr_end != &self.buf_data[0] {
                last = *self.ptr_end.offset(-1);
            } else {
                last = 0;
            }
            self.ptr_read = self.ptr_end;
            if !try!(self.read()) {
                return Ok(b"");
            }
        }
    }

    unsafe fn parse_directive(&mut self) -> Result<(), Error> {
        if self.done {
            try!(self.preload(0x400));
        }
        let block = self.block;
        self.next();
        try!(self.parse_spaces());
        let mut token = [0; 0x10];
        match &try!(self.parse_token(&mut token))[..] {
            b"line" => self.parse_directive_line(block),
            b"pragma" => self.parse_directive_pragma(),
            _ => {
                try!(self.next_line());
                Ok(())
            }
        }
    }

    unsafe fn parse_directive_line(&mut self, block: i32) -> Result<(), Error> {
        let mut line_token = [0; 0x10];
        let mut file_token = [0; 0x400];
        let mut file_raw = [0; 0x400];
        try!(self.parse_spaces());
        let line = try!(self.parse_token(&mut line_token));
        try!(self.parse_spaces());
        let (file, raw) = try!(self.parse_path(&mut file_token, &mut file_raw));
        let eol = try!(self.next_line_eol());
        if line == b"1" {
            if try!(self.writer.is_source_separator(file)) {
                self.done = false;
                self.header_found = false;
                self.entry_file = None;
                if !self.keep_headers {
                    // Skip current directive.
                    self.ptr_copy = self.ptr_read;
                }
                if self.block != block {
                    return Err(Error::new(ErrorKind::InvalidInput, PostprocessError::TokenTooLong));
                }
            }
        }
        self.entry_file = match self.entry_file.take() {
            Some(path) => {
                if self.header_found && (path == file) {
                    let mut mark = Vec::with_capacity(0x400);
                    try!(mark.write(b"#pragma hdrstop"));
                    try!(mark.write(&eol));
                    try!(mark.write(b"#line "));
                    try!(mark.write(&line));
                    try!(mark.write(b" "));
                    try!(mark.write(&raw));
                    try!(mark.write(&eol));
                    try!(self.write(&mark));
                    self.done = true;
                }
                match &self.marker {
                    &Some(ref path) => {
                        if is_subpath(&file, &path) {
                            self.header_found = true;
                        }
                    }
                    &None => {}
                }
                Some(path)
            }
            None => Some(Vec::from(file)),
        };
        Ok(())
    }

    unsafe fn parse_directive_pragma(&mut self) -> Result<(), Error> {
        try!(self.parse_spaces());
        let mut token = [0; 0x20];
        match &try!(self.parse_token(&mut token))[..] {
            b"hdrstop" => {
                if !self.done {
                    try!(self.flush());
                    if !self.keep_headers {
                        try!(self.write(b"#pragma hdrstop"));
                    }
                    self.done = true;
                }
            }
            _ => {
                try!(self.next_line());
            }
        }
        Ok(())
    }

    unsafe fn parse_escape(&mut self) -> Result<u8, Error> {
        self.next();
        match try!(self.peek()) {
            Some(c) => {
                self.next();
                match c {
                    b'n' => Ok(b'\n'),
                    b'r' => Ok(b'\r'),
                    b't' => Ok(b'\t'),
                    c => Ok(c),
                }
            }
            None => Err(Error::new(ErrorKind::InvalidInput, PostprocessError::EscapeEof)),
        }
    }

    unsafe fn parse_spaces(&mut self) -> Result<(), Error> {
        loop {
            while self.ptr_read != self.ptr_end {
                match *self.ptr_read {
                    // non-nl-white-space ::= a blank, tab, or formfeed character
                    b' ' | b'\t' | b'\x0C' => {
                        self.next();
                    }
                    _ => {
                        return Ok(());
                    }
                }
            }
            if !try!(self.read()) {
                return Ok(());
            }
        }
    }

    unsafe fn parse_empty(&mut self) -> Result<(), Error> {
        loop {
            while self.ptr_read != self.ptr_end {
                match *self.ptr_read {
                    // non-nl-white-space ::= a blank, tab, or formfeed character
                    b' ' | b'\t' | b'\x0C' | b'\n' | b'\r' => {
                        self.next();
                    }
                    _ => {
                        return Ok(());
                    }
                }
            }
            if !try!(self.read()) {
                return Ok(());
            }
        }
    }

    unsafe fn parse_token<'b>(&mut self, token: &'b mut [u8]) -> Result<&'b [u8], Error> {
        let mut offset: usize = 0;
        loop {
            while self.ptr_read != self.ptr_end {
                let c: u8 = *self.ptr_read;
                match c {
                    // end-of-line ::= newline | carriage-return | carriage-return newline
                    b'a'...b'z' | b'A'...b'Z' | b'0'...b'9' | b'_' => {
                        if offset >= token.len() {
                            return Err(Error::new(ErrorKind::InvalidInput, PostprocessError::TokenTooLong));
                        }
                        token[offset] = c;
                        offset += 1;
                    }
                    _ => {
                        return Ok(&token[0..offset]);
                    }
                }
                self.next();
            }
            if !try!(self.read()) {
                return Ok(token);
            }
        }
    }

    unsafe fn parse_path<'t, 'r>(&mut self,
                                 token: &'t mut [u8],
                                 raw: &'r mut [u8])
                                 -> Result<(&'t [u8], &'r [u8]), Error> {
        let quote = try!(self.peek()).unwrap();
        raw[0] = quote;
        self.next();
        let mut token_offset = 0;
        let mut raw_offset = 1;
        loop {
            while self.ptr_read != self.ptr_end {
                let c: u8 = *self.ptr_read;
                match c {
                    // end-of-line ::= newline | carriage-return | carriage-return newline
                    b'\n' | b'\r' => {
                        return Err(Error::new(ErrorKind::InvalidInput, PostprocessError::LiteralEol));
                    }
                    b'\\' => {
                        raw[raw_offset + 0] = b'\\';
                        raw[raw_offset + 1] = c;
                        raw_offset += 2;
                        token[token_offset] = match try!(self.parse_escape()) {
                            b'\\' => b'/',
                            v => v,
                        };
                        token_offset += 1;
                    }
                    c => {
                        self.next();
                        raw[raw_offset] = c;
                        raw_offset += 1;
                        if c == quote {
                            return Ok((&token[..token_offset], &raw[..raw_offset]));
                        }
                        token[token_offset] = c;
                        token_offset += 1;
                    }
                }
                if (raw_offset >= raw.len() - 2) || (token_offset >= token.len() - 1) {
                    return Err(Error::new(ErrorKind::InvalidInput, PostprocessError::LiteralTooLong));
                }
            }
            if !try!(self.read()) {
                return Err(Error::new(ErrorKind::InvalidInput, PostprocessError::LiteralEof));
            }
        }
    }
}

fn is_subpath(parent: &[u8], child: &[u8]) -> bool {
    if parent.len() < child.len() {
        return false;
    }
    if (parent.len() != child.len()) && (parent[parent.len() - child.len() - 1] != b'/') {
        return false;
    }
    child.eq_ignore_ascii_case(&parent[parent.len() - child.len()..])
}

unsafe fn delta(beg: *const u8, end: *const u8) -> usize {
    (end as usize) - (beg as usize)
}

#[cfg(test)]
mod test {
    use ::vs::postprocess::PostprocessWrite;

    use std::collections::HashSet;
    use std::io::{Cursor, Error, Write};

    struct OutputWrapper<'a> {
        content: Vec<u8>,
        sources: HashSet<&'a [u8]>,
        eol: &'a [u8],
    }

    impl<'a> Write for OutputWrapper<'a> {
        fn write(&mut self, buf: &[u8]) -> Result<usize, Error> {
            self.content.write(buf)
        }
        fn flush(&mut self) -> Result<(), Error> {
            self.content.flush()
        }
    }

    impl<'a> PostprocessWrite for OutputWrapper<'a> {
        fn is_source_separator(&mut self, marker: &[u8]) -> Result<bool, Error> {
            if self.sources.remove(marker) {
                try!(self.content.write(b"/// "));
                try!(self.content.write(marker));
                try!(self.content.write(self.eol));
                Ok(true)
            } else {
                Ok(false)
            }
        }
    }

    fn check_filter_pass(original: &str,
                         expected: &str,
                         sources: &[&str],
                         marker: &Option<String>,
                         keep_headers: bool,
                         eol: &str) {
        let mut stream: Vec<u8> = Vec::new();
        stream.write(&original.replace("\n", eol).as_bytes()[..]).unwrap();
        let mut wrapper = OutputWrapper {
            content: Vec::new(),
            sources: HashSet::new(),
            eol: eol.as_bytes(),
        };
        for source in sources.iter() {
            wrapper.sources.insert(source.as_bytes());
        }
        match super::filter_preprocessed(&mut Cursor::new(stream), &mut wrapper, marker, keep_headers) {
            Ok(_) => {
                let actual = String::from_utf8_lossy(&wrapper.content);
                if actual != expected.replace("\n", eol) {
                    println!("==== ACTUAL ====\n{}\n", actual.replace("\r", ""));
                    println!("=== EXPECTED ===\n{}\n", expected.replace("\r", ""));
                }
                assert_eq!(actual, expected.replace("\n", eol));
                assert_eq!(wrapper.sources.len(), 0);
            }
            Err(e) => {
                println!("{:?}", e);
                panic!(e);
            }
        }
    }

    fn check_filter(original: &str, expected: &str, source: &str, marker: Option<String>, keep_headers: bool) {
        check_filter_pass(original, expected, &[source], &marker, keep_headers, "\n");
        check_filter_pass(original, expected, &[source], &marker, keep_headers, "\r\n");

        let second = "./sample.foo.cpp";
        let original_multi = original.to_string() + &original.replace(source, second);
        let expected_multi = expected.to_string() + &expected.replace(source, second);
        check_filter_pass(&original_multi,
                          &expected_multi,
                          &[source, second],
                          &marker,
                          keep_headers,
                          "\n");
        check_filter_pass(&original_multi,
                          &expected_multi,
                          &[source, second],
                          &marker,
                          keep_headers,
                          "\r\n");
    }

    #[test]
    fn test_filter_precompiled_keep() {
        check_filter(r#"#line 1 "./sample.cpp"
#line 1 "e:/work/octobuild/test_cl/sample header.h"
# pragma once
void hello();
#line 2 "./sample.cpp"

int main(int argc, char **argv) {
	return 0;
}
"#,
                     r#"/// ./sample.cpp
#line 1 "./sample.cpp"
#line 1 "e:/work/octobuild/test_cl/sample header.h"
# pragma once
void hello();
#line 2 "./sample.cpp"
#pragma hdrstop
#line 2 "./sample.cpp"

int main(int argc, char **argv) {
	return 0;
}
"#,
                     "./sample.cpp",
                     Some("sample header.h".to_string()),
                     true)
    }

    #[test]
    fn test_filter_precompiled_remove() {
        check_filter(r#"#line 1 "sample.cpp"
#line 1 "e:/work/octobuild/test_cl/sample header.h"
# pragma once
void hello1();
void hello2();
#line 2 "sample.cpp"

int main(int argc, char **argv) {
	return 0;
}
"#,
                     r#"/// sample.cpp
#pragma hdrstop
#line 2 "sample.cpp"

int main(int argc, char **argv) {
	return 0;
}
"#,
                     "sample.cpp",
                     Some("sample header.h".to_string()),
                     false);
    }

    #[test]
    fn test_filter_precompiled_case() {
        check_filter(r#"#line 1 "sample.cpp"
#line 1 "e:/work/octobuild/test_cl/StdAfx.h"
# pragma once
void hello1();
void hello2();
#line 2 "sample.cpp"

int main(int argc, char **argv) {
    return 0;
}
"#,
                     r#"/// sample.cpp
#pragma hdrstop
#line 2 "sample.cpp"

int main(int argc, char **argv) {
    return 0;
}
"#,
                     "sample.cpp",
                     Some("STDafx.h".to_string()),
                     false);
    }

    #[test]
    fn test_filter_precompiled_hdrstop() {
        check_filter(r#"#line 1 "sample.cpp"
 #line 1 "e:/work/octobuild/test_cl/sample header.h"
void hello();
# pragma  hdrstop
void data();
# pragma once
#line 2 "sample.cpp"

int main(int argc, char **argv) {
	return 0;
}
"#,
                     r#"/// sample.cpp
#pragma hdrstop
void data();
# pragma once
#line 2 "sample.cpp"

int main(int argc, char **argv) {
	return 0;
}
"#,
                     "sample.cpp",
                     None,
                     false);
    }

    #[test]
    fn test_filter_precompiled_hdrstop_keep() {
        check_filter(r#"#line 1 "sample.cpp"
 #line 1 "e:/work/octobuild/test_cl/sample header.h"
void hello();
# pragma  hdrstop
void data();
# pragma once
#line 2 "sample.cpp"

int main(int argc, char **argv) {
	return 0;
}
"#,
                     r#"/// sample.cpp
#line 1 "sample.cpp"
 #line 1 "e:/work/octobuild/test_cl/sample header.h"
void hello();
# pragma  hdrstop
void data();
# pragma once
#line 2 "sample.cpp"

int main(int argc, char **argv) {
	return 0;
}
"#,
                     "sample.cpp",
                     None,
                     true);
    }

    #[test]
    fn test_filter_precompiled_winpath() {
        check_filter(r#"#line 1 "sample.cpp"
#line 1 "e:\\work\\octobuild\\test_cl\\sample header.h"
# pragma once
void hello();
#line 2 "sample.cpp"

int main(int argc, char **argv) {
	return 0;
}
"#,
                     r#"/// sample.cpp
#line 1 "sample.cpp"
#line 1 "e:\\work\\octobuild\\test_cl\\sample header.h"
# pragma once
void hello();
#line 2 "sample.cpp"
#pragma hdrstop
#line 2 "sample.cpp"

int main(int argc, char **argv) {
	return 0;
}
"#,
                     "sample.cpp",
                     Some("e:\\work\\octobuild\\test_cl\\sample header.h".to_string()),
                     true);
    }
}
