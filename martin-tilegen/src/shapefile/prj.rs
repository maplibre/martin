//! The coordinate system a shapefile's `.prj` describes, as far as Martin reads one: WGS84 or Web Mercator.

use crate::source::Crs;

/// EPSG codes that are Web Mercator, with the ESRI codes for it.
const WEB_MERCATOR_CODES: [&str; 5] = ["3857", "900913", "3785", "102100", "102113"];

/// Projection names, normalized by [`normalize`], that are Web Mercator on a sphere.
const WEB_MERCATOR_PROJECTIONS: [&str; 3] = [
    "mercatorauxiliarysphere",
    "popularvisualisationpseudomercator",
    "pseudomercator",
];

/// One node of WKT, such as `DATUM["WGS_1984", SPHEROID[...]]`.
#[derive(Debug, PartialEq)]
struct Node {
    keyword: String,
    args: Vec<Arg>,
}

/// One argument of a WKT node.
#[derive(Debug, PartialEq)]
enum Arg {
    /// A quoted string.
    Text(String),
    /// A number or a bare word, such as `EAST`.
    Word(String),
    /// A nested node.
    Node(Node),
}

impl Node {
    /// The first quoted argument, the node's name in WKT1.
    fn name(&self) -> Option<&str> {
        self.args.iter().find_map(|arg| match arg {
            Arg::Text(text) => Some(text.as_str()),
            Arg::Word(_) | Arg::Node(_) => None,
        })
    }

    /// The direct child nodes with `keyword`, in any case.
    fn children<'a>(&'a self, keyword: &'a str) -> impl Iterator<Item = &'a Self> + 'a {
        self.args.iter().filter_map(move |arg| match arg {
            Arg::Node(node) if node.keyword.eq_ignore_ascii_case(keyword) => Some(node),
            Arg::Text(_) | Arg::Word(_) | Arg::Node(_) => None,
        })
    }

    /// The EPSG code of the node's own `AUTHORITY` or `ID`, if it has one.
    fn epsg(&self) -> Option<String> {
        self.children("AUTHORITY").chain(self.children("ID")).find_map(|node| {
            let mut args = node.args.iter();
            let (Some(Arg::Text(authority)), Some(Arg::Text(code) | Arg::Word(code))) = (args.next(), args.next())
            else {
                return None;
            };
            authority.eq_ignore_ascii_case("EPSG").then(|| code.clone())
        })
    }

    /// The value of the `PARAMETER` named `name`, in any spelling [`normalize`] equates.
    fn parameter(&self, name: &str) -> Option<f64> {
        self.children("PARAMETER")
            .find(|node| node.name().is_some_and(|n| normalize(n) == name))
            .and_then(|node| {
                node.args.iter().find_map(|arg| match arg {
                    Arg::Word(word) => word.parse().ok(),
                    Arg::Text(_) | Arg::Node(_) => None,
                })
            })
    }
}

/// The name in lower case without anything but letters and digits, so `WGS_1984` and `WGS 1984` match.
fn normalize(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Why a `.prj` is not one Martin reads.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Unsupported {
    /// The text is not WKT.
    NotWkt,
    /// WKT of a coordinate system other than WGS84 and Web Mercator, by its name.
    Other(String),
}

/// The coordinate system that the WKT `text` of a `.prj` describes.
pub(super) fn crs(text: &str) -> Result<Crs, Unsupported> {
    let node = parse(text).ok_or(Unsupported::NotWkt)?;
    let epsg = node.epsg();
    let crs = match node.keyword.to_ascii_uppercase().as_str() {
        "GEOGCS" | "GEOGCRS" | "GEODCRS" => match epsg.as_deref() {
            Some(code) => (code == "4326").then_some(Crs::Wgs84),
            None => is_wgs84(&node).then_some(Crs::Wgs84),
        },
        "PROJCS" | "PROJCRS" => match epsg.as_deref() {
            Some(code) => WEB_MERCATOR_CODES.contains(&code).then_some(Crs::WebMercator),
            None => is_web_mercator(&node).then_some(Crs::WebMercator),
        },
        _ => None,
    };
    crs.ok_or_else(|| Unsupported::Other(node.name().unwrap_or(&node.keyword).to_owned()))
}

/// Whether the WKT1 `GEOGCS` is WGS84 in degrees from Greenwich.
fn is_wgs84(geogcs: &Node) -> bool {
    let datum = geogcs.children("DATUM").next();
    let wgs84 = datum.is_some_and(|datum| {
        datum.epsg().as_deref() == Some("6326")
            || datum
                .name()
                .is_some_and(|name| matches!(normalize(name).as_str(), "wgs1984" | "dwgs1984" | "wgs84"))
    });
    let greenwich = geogcs.children("PRIMEM").next().is_none_or(|primem| {
        primem.args.iter().any(|arg| match arg {
            Arg::Word(value) => value.parse::<f64>() == Ok(0.0),
            Arg::Text(_) | Arg::Node(_) => false,
        })
    });
    wgs84 && greenwich
}

/// Whether the WKT1 `PROJCS` is Web Mercator: a spherical Mercator of WGS84 without offsets, in metres.
fn is_web_mercator(projcs: &Node) -> bool {
    let projection = projcs
        .children("PROJECTION")
        .next()
        .and_then(Node::name)
        .is_some_and(|name| WEB_MERCATOR_PROJECTIONS.contains(&normalize(name).as_str()));
    let unshifted = [
        "falseeasting",
        "falsenorthing",
        "centralmeridian",
        "auxiliaryspheretype",
    ]
    .iter()
    .all(|name| projcs.parameter(name).is_none_or(|value| value == 0.0));
    let metres = projcs.children("UNIT").next().is_none_or(|unit| {
        unit.args.iter().any(|arg| match arg {
            Arg::Word(value) => value.parse::<f64>() == Ok(1.0),
            Arg::Text(_) | Arg::Node(_) => false,
        })
    });
    let wgs84 = projcs.children("GEOGCS").next().is_some_and(is_wgs84);
    projection && unshifted && metres && wgs84
}

/// The single root node of WKT `text`, or `None` if it is not WKT.
fn parse(text: &str) -> Option<Node> {
    let mut parser = Parser {
        chars: text.trim_start_matches('\u{feff}').chars().collect(),
        pos: 0,
    };
    let node = parser.node()?;
    parser.skip_whitespace();
    (parser.pos == parser.chars.len()).then_some(node)
}

/// A WKT reader at a position of its text.
struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    /// Reads one node, `KEYWORD[arg, ...]` or with parentheses.
    fn node(&mut self) -> Option<Node> {
        self.skip_whitespace();
        let keyword = self.word();
        if keyword.is_empty() {
            return None;
        }
        self.arguments(keyword)
    }

    /// Reads the bracketed arguments of the node `keyword`.
    fn arguments(&mut self, keyword: String) -> Option<Node> {
        self.skip_whitespace();
        let close = match self.next()? {
            '[' => ']',
            '(' => ')',
            _ => return None,
        };
        let mut args = Vec::new();
        loop {
            self.skip_whitespace();
            if self.peek() == Some('"') {
                args.push(Arg::Text(self.text()?));
            } else {
                let word = self.word();
                if word.is_empty() {
                    return None;
                }
                self.skip_whitespace();
                if matches!(self.peek(), Some('[' | '(')) {
                    args.push(Arg::Node(self.arguments(word)?));
                } else {
                    args.push(Arg::Word(word));
                }
            }
            self.skip_whitespace();
            match self.next()? {
                ',' => {}
                c if c == close => return Some(Node { keyword, args }),
                _ => return None,
            }
        }
    }

    /// Reads a quoted string, in which `""` stands for one quote.
    fn text(&mut self) -> Option<String> {
        self.next();
        let mut text = String::new();
        loop {
            match self.next()? {
                '"' if self.peek() == Some('"') => {
                    self.next();
                    text.push('"');
                }
                '"' => return Some(text),
                c => text.push(c),
            }
        }
    }

    /// Reads a keyword, a bare word or a number.
    fn word(&mut self) -> String {
        let mut word = String::new();
        while let Some(c) = self
            .peek()
            .filter(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '-' | '+'))
        {
            word.push(c);
            self.pos += 1;
        }
        word
    }

    /// Skips spaces and line breaks.
    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
    }

    /// The character at the position, if any.
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    /// Takes the character at the position, if any.
    fn next(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        Some(c)
    }
}
