//! Parse XML dumps exported from Mediawiki.
//!
//! This module parses [XML dumps](https://www.mediawiki.org/wiki/Help:Export) exported from Mediawiki, providing each page from the dump through an iterator. This is useful for parsing the [dumps from Wikipedia and other Wikimedia projects](https://dumps.wikimedia.org).
//!
//! # Caution
//!
//! If you need to parse any wiki text extracted from a dump, please use the crate [Parse Wiki Text](https://github.com/portstrom/parse_wiki_text). Correctly parsing wiki text requires dealing with an astonishing amount of difficult and counterintuitive cases. Parse Wiki Text automatically deals with all these cases, giving you an unambiguous parsed tree that is easy to work with.
//!
//! # Limitations
//!
//! This module only parses dumps containing only one revision of each page. This is what you get from the page `Special:Export` when enabling the option “Include only the current revision, not the full history”, as well as what you get from the Wikimedia dumps with file names ending with `-pages-articles.xml.bz2`.
//!
//! This module ignores the `siteinfo` element, every child element of the `page` element except `ns`, `revision` and `title`, and every element inside the `revision` element except `format`, `model` and `text`.
//!
//! Until there is a real use case that justifies going beyond these limitations, they will remain in order to avoid premature design driven by imagined requirements.
//!
//! # Examples
//!
//! Parse a bzip2 compressed file and distinguish ordinary articles from other pages. An running example with complete error handling is available in the `examples` folder.
//!
//! ```rust,no_run
//! extern crate bzip2;
//! extern crate parse_mediawiki_dump;
//!
//! fn main() {
//!     let file = std::fs::File::open("example.xml.bz2").unwrap();
//!     let file = bzip2::read::BzDecoder::new(file);
//!     for result in parse_mediawiki_dump::parse(file) {
//!         match result {
//!             Err(error) => {
//!                 eprintln!("Error: {}", error);
//!                 break;
//!             }
//!             Ok(page) => if page.namespace == 0 && match &page.format {
//!                 None => false,
//!                 Some(format) => format == "text/x-wiki"
//!             } && match &page.model {
//!                 None => false,
//!                 Some(model) => model == "wikitext"
//!             } {
//!                 println!(
//!                     "The page {title:?} is an ordinary article with byte length {length}.",
//!                     title = page.title,
//!                     length = page.text.len()
//!                 );
//!             } else {
//!                 println!("The page {:?} has something special to it.", page.title);
//!             }
//!         }
//!     }
//! }
//! ```

#![warn(missing_docs)]

extern crate xml;

use std::io::Read;
use xml::{common::{Position, TextPosition}, reader::{EventReader, XmlEvent}};

#[derive(Debug)]
/// The error type for `Parser`.
pub enum Error {
    /// Format not matching expectations.
    ///
    /// Indicates the position in the stream.
    Format(TextPosition),

    /// The source contains a feature not supported by the parser.
    ///
    /// In particular, this means a `page` element contains more than one `revision` element.
    NotSupported(TextPosition),

    /// Error from the XML reader.
    XmlReader(xml::reader::Error)
}

/// Parsed page.
///
/// Parsed from the `page` element.
///
/// Although the `format` and `model` elements are defined as mandatory in the [schema](https://www.mediawiki.org/xml/export-0.10.xsd), previous versions of the schema don't contain them. Therefore the corresponding fields can be `None`.
#[derive(Debug)]
pub struct Page {
    /// The format of the revision if any.
    ///
    /// Parsed from the text content of the `format` element in the `revision` element. `None` if the element is not present.
    ///
    /// For ordinary articles the format is `text/x-wiki`.
    pub format: Option<String>,

    /// The model of the revision if any.
    ///
    /// Parsed from the text content of the `model` element in the `revision` element. `None` if the element is not present.
    ///
    /// For ordinary articles the model is `wikitext`.
    pub model: Option<String>,

    /// The namespace of the page.
    ///
    /// Parsed from the text content of the `ns` element in the `page` element.
    ///
    /// For ordinary articles the namespace is 0.
    pub namespace: u32,

    /// The text of the revision.
    ///
    /// Parsed from the text content of the `text` element in the `revision` element.
    pub text: String,

    /// The title of the page.
    ///
    /// Parsed from the text content of the `title` element in the `page` element.
    pub title: String
}

/// Parser working as an iterator over pages.
pub struct Parser<R: Read> {
    event_reader: ::EventReader<R>,
    started: bool
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Format(position) => write!(formatter, "Invalid format at position {}", position),
            Error::NotSupported(position) => write!(formatter, "The element at position {} is not supported", position),
            Error::XmlReader(error) => error.fmt(formatter)
        }
    }
}

impl From<xml::reader::Error> for Error {
    fn from(value: xml::reader::Error) -> Self {
        Error::XmlReader(value)
    }
}

impl<R: Read> Iterator for Parser<R> {
    type Item = Result<Page, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        Some(match next(self) {
            Err(error) => Err(error),
            Ok(item) => Ok(item?)
        })
    }
}

fn match_namespace(name: &xml::name::OwnedName) -> bool {
    match &name.namespace {
        None => false,
        Some(namespace) => namespace == "http://www.mediawiki.org/xml/export-0.10/"
    }
}

fn next(parser: &mut Parser<impl Read>) -> Result<Option<Page>, Error> {
    if !parser.started {
        loop {
            if let XmlEvent::StartElement { name, .. } = parser.event_reader.next()? {
                if match_namespace(&name) && name.local_name == "mediawiki" {
                    break;
                }
                return Err(Error::Format(parser.event_reader.position()));
            }
        }
        parser.started = true;
    }
    loop {
        match parser.event_reader.next()? {
            XmlEvent::EndElement { .. } => return Ok(None),
            XmlEvent::StartElement { name, .. } => if match &name.namespace {
                None => false,
                Some(namespace) => namespace == "http://www.mediawiki.org/xml/export-0.10/"
            } && name.local_name == "page" {
                let mut format = None;
                let mut model = None;
                let mut namespace = None;
                let mut text = None;
                let mut title = None;
                loop {
                    match parser.event_reader.next()? {
                        XmlEvent::EndElement { .. } => return match (namespace, text, title) {
                            (Some(namespace), Some(text), Some(title)) => Ok(Some(Page { format, model, namespace, text, title })),
                            _ => Err(Error::Format(parser.event_reader.position()))
                        },
                        XmlEvent::StartElement { name, .. } => {
                            if match &name.namespace {
                                None => false,
                                Some(namespace) => namespace == "http://www.mediawiki.org/xml/export-0.10/"
                            } {
                                match &name.local_name as _ {
                                    "ns" => match parse_text(&mut parser.event_reader, &namespace)?.parse() {
                                        Err(_) => return Err(Error::Format(parser.event_reader.position())),
                                        Ok(value) => {
                                            namespace = Some(value);
                                            continue;
                                        }
                                    }
                                    "revision" => {
                                        if text.is_some() {
                                            return Err(Error::NotSupported(parser.event_reader.position()));
                                        }
                                        loop {
                                            match parser.event_reader.next()? {
                                                XmlEvent::EndElement { .. } => match text {
                                                    None => return Err(Error::Format(parser.event_reader.position())),
                                                    Some(_) => break
                                                }
                                                XmlEvent::StartElement { name, .. } => {
                                                    if match &name.namespace {
                                                        None => false,
                                                        Some(namespace) => namespace == "http://www.mediawiki.org/xml/export-0.10/"
                                                    } {
                                                        match &name.local_name as _ {
                                                            "format" => {
                                                                format = Some(parse_text(&mut parser.event_reader, &mut format)?);
                                                                continue;
                                                            }
                                                            "model" => {
                                                                model = Some(parse_text(&mut parser.event_reader, &mut model)?);
                                                                continue;
                                                            }
                                                            "text" => {
                                                                text = Some(parse_text(&mut parser.event_reader, &mut text)?);
                                                                continue;
                                                            }
                                                            _ => {}
                                                        }
                                                    }
                                                    skip_element(&mut parser.event_reader);
                                                }
                                                _ => {}
                                            }
                                        }
                                        continue;
                                    }
                                    "title" => {
                                        title = Some(parse_text(&mut parser.event_reader, &title)?);
                                        continue;
                                    }
                                    _ => {}
                                }
                            }
                            skip_element(&mut parser.event_reader);
                        }
                        _ => {}
                    }
                }
            } else {
                skip_element(&mut parser.event_reader);
            }
            _ => {}
        }
    }
}

/// Creates a parser for a stream.
///
/// The stream is parsed as an XML dump exported from Mediawiki. The parser is an iterator over the pages in the dump.
pub fn parse<R: Read>(source: R) -> Parser<R> {
    Parser {
        event_reader: EventReader::new(source),
        started: false
    }
}

fn parse_text(
    event_reader: &mut EventReader<impl Read>,
    output: &Option<impl Sized>
) -> Result<String, Error> {
    if output.is_some() {
        return Err(Error::Format(event_reader.position()));
    }
    match event_reader.next()? {
        XmlEvent::Characters(characters) => if let XmlEvent::EndElement { .. } = event_reader.next()? {
            return Ok(characters);
        },
        XmlEvent::EndElement { .. } => return Ok(String::new()),
        _ => {}
    }
    Err(Error::Format(event_reader.position()))
}

fn skip_element(event_reader: &mut EventReader<impl Read>) {
    let mut level = 0;
    while let Ok(event) = event_reader.next() {
        match event {
            XmlEvent::EndElement { .. } => {
                if level == 0 {
                    return;
                }
                level -= 1;
            }
            XmlEvent::StartElement { .. } => level += 1,
            _ => {}
        }
    }
}
