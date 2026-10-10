//! Shared pieces of every Vellum export target.
//!
//! A [`Document`](vellum_schema::Document) is lowered once into a
//! target-neutral element tree ([`lower::lower_document`]), and each
//! [`Exporter`] prints that tree in its own syntax. Targets live in their own
//! `export-*` crates and are collected in an [`ExporterRegistry`].

pub mod exporter;
pub mod lower;

pub use exporter::*;
