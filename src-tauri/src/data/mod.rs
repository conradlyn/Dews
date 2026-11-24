//! Ported data layer from the WPF edition of ReadYou Desktop.
//! Fever is deliberately absent; only Local RSS/Atom and Google Reader
//! compatible services (FreshRSS) are supported.

pub mod model;
pub mod xml;
pub mod html;
pub mod thumbnail;
pub mod rss;
pub mod opml;
pub mod article_html;
pub mod fulltext;
pub mod images;
pub mod fonts;
pub mod greader;
pub mod store;

#[cfg(test)]
mod protocol_tests;
