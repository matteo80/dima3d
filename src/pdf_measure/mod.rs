//! Misure da layout in PDF: si calibra la scala su una distanza nota e si misurano Delta X,
//! Delta Y e distanza in mm. Il PDF è disegnato a tile da pdfium (`tools\pdfium.dll`) in un
//! thread dedicato. La finestra è in `ui::pdf_panel`.
//!
//! Stesso codice del programma separato «PDF Layout Measure»: le modifiche al widget vanno
//! riportate in entrambi.

// Parte dell'API (getter, dati di diagnostica) serve solo al programma separato e ai test.
#![allow(dead_code)]

pub mod measure;
pub mod render;
pub mod tiles;
mod viewer;

pub use viewer::PdfMeasure;
