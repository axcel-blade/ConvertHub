//! PDF tools: merge (native), text/Word/Excel/HTML conversion and image
//! extraction. Poppler and LibreOffice are used when installed because they
//! keep more layout; otherwise native Rust fallbacks produce text-based output.
//! OCR is not implemented: scanned (image-only) PDFs yield no text.

use crate::deps::{self, Tool};
use crate::error::{R, UiMsg};
use crate::ffmpeg::run_process;
use crate::fsutil::{file_stem, TempGuard};
use crate::jobs::{Ctx, Operation as Op};
use crate::ui;
use lopdf::{Document, Object, ObjectId};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

const TOOL_TIMEOUT: Option<Duration> = Some(Duration::from_secs(900));

pub fn run(ctx: &Ctx, op: &Op, inputs: &[PathBuf], out: &Path) -> R<Vec<PathBuf>> {
    match op {
        Op::PdfMerge => merge(ctx, inputs, out)?,
        Op::PdfConvert { target } => convert(ctx, &inputs[0], target, out)?,
        Op::PdfImages { as_jpg } => return extract_images(ctx, &inputs[0], *as_jpg, out),
        _ => unreachable!(),
    }
    ctx.progress(1.0);
    Ok(vec![out.to_path_buf()])
}

fn load(path: &Path) -> R<Document> {
    let doc = Document::load(path).map_err(|e| ui!("errors.pdfRead", "path" => path.display(), "detail" => e))?;
    if doc.is_encrypted() {
        return Err(ui!("errors.pdfEncrypted", "path" => path.display()));
    }
    Ok(doc)
}

/// Merge PDFs by renumbering each document's objects into one id space and
/// re-parenting every page under a single page tree.
pub fn merge(ctx: &Ctx, inputs: &[PathBuf], out: &Path) -> R<()> {
    ctx.set_engine("Native (lopdf)");
    let mut max_id = 1;
    let mut pages: BTreeMap<ObjectId, Object> = BTreeMap::new();
    let mut page_order: Vec<ObjectId> = vec![];
    let mut objects: BTreeMap<ObjectId, Object> = BTreeMap::new();
    let mut merged = Document::with_version("1.5");

    for (i, path) in inputs.iter().enumerate() {
        ctx.check()?;
        let mut doc = load(path)?;
        doc.renumber_objects_with(max_id);
        max_id = doc.max_id + 1;
        for (_, id) in doc.get_pages() {
            pages.insert(id, doc.get_object(id)?.to_owned());
            page_order.push(id);
        }
        objects.extend(doc.objects);
        ctx.progress(0.7 * (i + 1) as f64 / inputs.len() as f64);
    }

    let mut catalog: Option<(ObjectId, Object)> = None;
    let mut pages_root: Option<(ObjectId, Object)> = None;
    for (id, object) in objects {
        match object.type_name().unwrap_or(b"") {
            b"Catalog" => {
                if catalog.is_none() {
                    catalog = Some((id, object));
                }
            }
            b"Pages" => {
                if let Ok(dict) = object.as_dict() {
                    let mut dict = dict.clone();
                    let root_id = match &pages_root {
                        Some((rid, old)) => {
                            if let Ok(old) = old.as_dict() {
                                dict.extend(old);
                            }
                            *rid
                        }
                        None => id,
                    };
                    pages_root = Some((root_id, Object::Dictionary(dict)));
                }
            }
            b"Page" | b"Outlines" | b"Outline" => {}
            _ => {
                merged.objects.insert(id, object);
            }
        }
    }
    let (pages_id, pages_obj) = pages_root.ok_or_else(|| ui!("errors.pdfStructure"))?;
    let (catalog_id, catalog_obj) = catalog.ok_or_else(|| ui!("errors.pdfStructure"))?;

    for (id, object) in &pages {
        if let Ok(dict) = object.as_dict() {
            let mut dict = dict.clone();
            dict.set("Parent", pages_id);
            merged.objects.insert(*id, Object::Dictionary(dict));
        }
    }
    let mut pd = pages_obj.as_dict()?.clone();
    pd.set("Count", page_order.len() as u32);
    pd.set("Kids", page_order.iter().map(|id| Object::Reference(*id)).collect::<Vec<_>>());
    pd.remove(b"Parent");
    merged.objects.insert(pages_id, Object::Dictionary(pd));

    let mut cd = catalog_obj.as_dict()?.clone();
    cd.set("Pages", pages_id);
    cd.remove(b"Outlines");
    merged.objects.insert(catalog_id, Object::Dictionary(cd));
    merged.trailer.set("Root", catalog_id);
    merged.max_id = merged.objects.len() as u32;
    merged.renumber_objects();
    merged.compress();

    let tmp = TempGuard::file_for(out);
    merged.save(&tmp.path)?;
    tmp.commit(out)
}

/// Page texts, preferring Poppler's layout-preserving `pdftotext`.
fn page_texts(ctx: &Ctx, input: &Path) -> R<Vec<String>> {
    if let Some(pt) = deps::find(Tool::Pdftotext) {
        let tmp = TempGuard::in_system_temp()?;
        let txt = tmp.path.join("out.txt");
        let args: Vec<OsString> = vec!["-layout".into(), "-enc".into(), "UTF-8".into(), input.into(), txt.clone().into()];
        run_process(ctx, &pt, &args, |_| {}, TOOL_TIMEOUT)?;
        ctx.set_engine("Poppler (pdftotext)");
        let text = String::from_utf8_lossy(&std::fs::read(&txt)?).to_string();
        let mut pages: Vec<String> = text.split('\x0c').map(String::from).collect();
        if pages.last().is_some_and(|p| p.trim().is_empty()) {
            pages.pop();
        }
        return Ok(pages);
    }
    load(input)?; // clear errors for encrypted / unreadable files
    let path = input.to_path_buf();
    // pdf-extract can panic on malformed fonts; contain it.
    let pages = std::panic::catch_unwind(move || pdf_extract::extract_text_by_pages(&path))
        .map_err(|_| ui!("errors.pdfTextFailed"))?
        .map_err(|e| ui!("errors.pdfRead", "path" => input.display(), "detail" => e))?;
    ctx.set_engine("Native (pdf-extract)");
    Ok(pages)
}

fn require_text(pages: &[String]) -> R<()> {
    if pages.iter().all(|p| p.trim().is_empty()) {
        Err(ui!("errors.pdfNoText"))
    } else {
        Ok(())
    }
}

fn soffice_convert(ctx: &Ctx, input: &Path, filter: &str, import_pdf: bool) -> R<(TempGuard, PathBuf)> {
    let so = deps::require(Tool::Soffice)?;
    let tmp = TempGuard::in_system_temp()?;
    let profile = tmp.path.join("profile");
    let profile_url = format!("file:///{}", profile.display().to_string().replace('\\', "/").trim_start_matches('/'));
    let mut args: Vec<OsString> = vec![format!("-env:UserInstallation={profile_url}").into(), "--headless".into()];
    if import_pdf {
        args.push("--infilter=writer_pdf_import".into());
    }
    args.extend(["--convert-to".into(), filter.into(), "--outdir".into(), tmp.path.clone().into(), input.into()]);
    run_process(ctx, &so, &args, |_| {}, TOOL_TIMEOUT)?;
    ctx.set_engine("LibreOffice");
    let ext = filter.split(':').next().unwrap_or(filter);
    let produced = tmp.path.join(format!("{}.{ext}", input.file_stem().unwrap_or_default().to_string_lossy()));
    if !produced.is_file() {
        return Err(ui!("errors.noOutput"));
    }
    Ok((tmp, produced))
}

fn convert(ctx: &Ctx, input: &Path, target: &str, out: &Path) -> R<()> {
    let tmp = TempGuard::file_for(out);
    ctx.stage(UiMsg::new("stage.extracting"));
    match target {
        "txt" => {
            let pages = page_texts(ctx, input)?;
            require_text(&pages)?;
            std::fs::write(&tmp.path, pages.join("\n\n"))?;
        }
        "html" | "htm" => {
            if let Some(ph) = deps::find(Tool::Pdftohtml) {
                let work = TempGuard::in_system_temp()?;
                let base = work.path.join("doc");
                let args: Vec<OsString> =
                    vec!["-s".into(), "-noframes".into(), "-i".into(), "-enc".into(), "UTF-8".into(), input.into(), base.into()];
                run_process(ctx, &ph, &args, |_| {}, TOOL_TIMEOUT)?;
                let html = std::fs::read_dir(&work.path)?
                    .filter_map(|e| e.ok().map(|e| e.path()))
                    .find(|p| p.extension().is_some_and(|e| e == "html"))
                    .ok_or_else(|| ui!("errors.noOutput"))?;
                std::fs::copy(html, &tmp.path)?;
                ctx.set_engine("Poppler (pdftohtml)");
            } else {
                let pages = page_texts(ctx, input)?;
                require_text(&pages)?;
                std::fs::write(&tmp.path, text_to_html(&file_stem(input), &pages))?;
            }
        }
        "docx" => {
            if deps::find(Tool::Soffice).is_some() {
                let (_keep, produced) = soffice_convert(ctx, input, "docx:MS Word 2007 XML", true)?;
                std::fs::copy(produced, &tmp.path)?;
            } else {
                let pages = page_texts(ctx, input)?;
                require_text(&pages)?;
                write_docx(&pages, &tmp.path)?;
                ctx.stage(UiMsg::new("result.textOnly"));
            }
        }
        "doc" => {
            let (_keep, produced) = soffice_convert(ctx, input, "doc:MS Word 97", true)?;
            std::fs::copy(produced, &tmp.path)?;
        }
        "xlsx" | "xls" => {
            let pages = table_pages(ctx, input)?;
            if pages.iter().all(|p| p.is_empty()) {
                return Err(ui!("errors.pdfNoText"));
            }
            if target == "xlsx" {
                write_xlsx(&pages, &tmp.path)?;
            } else {
                let work = TempGuard::in_system_temp()?;
                let xlsx = work.path.join(format!("{}.xlsx", file_stem(input)));
                write_xlsx(&pages, &xlsx)?;
                let (_keep, produced) = soffice_convert(ctx, &xlsx, "xls:MS Excel 97", false)?;
                std::fs::copy(produced, &tmp.path)?;
            }
            ctx.stage(UiMsg::new("result.tablesApprox"));
        }
        other => return Err(ui!("errors.unsupportedOutput", "format" => other)),
    }
    tmp.commit(out)
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub fn text_to_html(title: &str, pages: &[String]) -> String {
    let mut h = format!(
        "<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>{}</title>\
         <style>body{{font-family:sans-serif;max-width:60em;margin:2em auto}}section{{border-bottom:1px solid #ccc;padding:1em 0}}pre{{white-space:pre-wrap;font-family:inherit}}</style>\
         </head><body>\n",
        escape_html(title)
    );
    for (i, p) in pages.iter().enumerate() {
        h.push_str(&format!("<section id=\"page-{}\"><pre>{}</pre></section>\n", i + 1, escape_html(p.trim_end())));
    }
    h.push_str("</body></html>\n");
    h
}

fn write_docx(pages: &[String], out: &Path) -> R<()> {
    use docx_rs::{BreakType, Docx, Paragraph, Run};
    let mut doc = Docx::new();
    for (i, page) in pages.iter().enumerate() {
        if i > 0 {
            doc = doc.add_paragraph(Paragraph::new().add_run(Run::new().add_break(BreakType::Page)));
        }
        for line in page.lines() {
            doc = doc.add_paragraph(Paragraph::new().add_run(Run::new().add_text(line.trim_end())));
        }
    }
    let file = std::fs::File::create(out)?;
    doc.build().pack(file).map_err(crate::error::generic)?;
    Ok(())
}

/// Split a layout-preserved text line into cells on runs of 2+ spaces/tabs.
pub fn split_cells(line: &str) -> Vec<String> {
    let mut cells = vec![];
    let mut cur = String::new();
    let mut spaces = 0;
    for c in line.trim().chars() {
        if c == ' ' || c == '\t' {
            spaces += if c == '\t' { 2 } else { 1 };
            continue;
        }
        if spaces >= 2 && !cur.is_empty() {
            cells.push(std::mem::take(&mut cur));
        } else if spaces > 0 {
            cur.push(' ');
        }
        spaces = 0;
        cur.push(c);
    }
    if !cur.is_empty() {
        cells.push(cur);
    }
    cells
}

type Grid = Vec<Vec<String>>;

/// Rows of cells per page. With Poppler, cells come from word bounding
/// boxes (columns are split on wide horizontal gaps, which survives tightly
/// packed tables); otherwise from layout text split on runs of spaces.
fn table_pages(ctx: &Ctx, input: &Path) -> R<Vec<Grid>> {
    if let Some(pt) = deps::find(Tool::Pdftotext) {
        let tmp = TempGuard::in_system_temp()?;
        let html = tmp.path.join("bbox.html");
        let args: Vec<OsString> = vec!["-bbox".into(), "-enc".into(), "UTF-8".into(), input.into(), html.clone().into()];
        run_process(ctx, &pt, &args, |_| {}, TOOL_TIMEOUT)?;
        ctx.set_engine("Poppler (pdftotext -bbox)");
        return Ok(grids_from_bbox(&String::from_utf8_lossy(&std::fs::read(&html)?)));
    }
    let pages = page_texts(ctx, input)?;
    Ok(pages
        .iter()
        .map(|p| p.lines().filter(|l| !l.trim().is_empty()).map(split_cells).collect())
        .collect())
}

struct Word {
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
    text: String,
}

fn attr(tag: &str, name: &str) -> Option<f64> {
    let i = tag.find(&format!("{name}=\""))? + name.len() + 2;
    tag[i..].split('"').next()?.parse().ok()
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&amp;", "&")
}

/// Parse `pdftotext -bbox` output into per-page grids.
pub fn grids_from_bbox(html: &str) -> Vec<Grid> {
    let mut pages = vec![];
    for page in html.split("<page ").skip(1) {
        let mut words: Vec<Word> = page
            .split("<word ")
            .skip(1)
            .filter_map(|w| {
                let (tag, rest) = w.split_once('>')?;
                let text = unescape(rest.split("</word>").next()?);
                Some(Word { x0: attr(tag, "xMin")?, x1: attr(tag, "xMax")?, y0: attr(tag, "yMin")?, y1: attr(tag, "yMax")?, text })
            })
            .collect();
        words.sort_by(|a, b| a.y0.total_cmp(&b.y0).then(a.x0.total_cmp(&b.x0)));
        // Group into rows: words whose vertical centers are within half a line.
        let mut rows: Vec<Vec<Word>> = vec![];
        for w in words {
            let cy = (w.y0 + w.y1) / 2.0;
            let h = (w.y1 - w.y0).max(1.0);
            match rows.last_mut() {
                Some(r) if (cy - (r[0].y0 + r[0].y1) / 2.0).abs() < h / 2.0 => r.push(w),
                _ => rows.push(vec![w]),
            }
        }
        let grid = rows
            .into_iter()
            .map(|mut r| {
                r.sort_by(|a, b| a.x0.total_cmp(&b.x0));
                let mut cells: Vec<String> = vec![];
                let mut prev_x1: Option<f64> = None;
                for w in r {
                    let h = (w.y1 - w.y0).max(1.0);
                    // A normal word space is ~0.25-0.3 em; a column gap is wider.
                    let new_cell = prev_x1.is_none_or(|x1| w.x0 - x1 > h * 0.6);
                    if new_cell {
                        cells.push(w.text);
                    } else if let Some(c) = cells.last_mut() {
                        c.push(' ');
                        c.push_str(&w.text);
                    }
                    prev_x1 = Some(w.x1);
                }
                cells
            })
            .collect();
        pages.push(grid);
    }
    pages
}

fn write_xlsx(pages: &[Grid], out: &Path) -> R<()> {
    let mut wb = rust_xlsxwriter::Workbook::new();
    for (i, page) in pages.iter().enumerate() {
        let ws = wb.add_worksheet();
        ws.set_name(format!("Page {}", i + 1))?;
        for (row, cells) in page.iter().enumerate() {
            for (col, cell) in cells.iter().enumerate().take(16_000) {
                let (row, col) = (row as u32, col as u16);
                match cell.replace(',', "").parse::<f64>() {
                    Ok(n) if n.is_finite() => ws.write_number(row, col, n)?,
                    _ => ws.write_string(row, col, cell)?,
                };
            }
        }
    }
    wb.save(out)?;
    Ok(())
}

fn extract_images(ctx: &Ctx, input: &Path, as_jpg: bool, out_dir: &Path) -> R<Vec<PathBuf>> {
    let tmp = TempGuard::dir_for(out_dir)?;
    let stem = file_stem(input);
    if let Some(pi) = deps::find(Tool::Pdfimages) {
        let args: Vec<OsString> = vec!["-all".into(), input.into(), tmp.path.join(&stem).into()];
        run_process(ctx, &pi, &args, |_| {}, TOOL_TIMEOUT)?;
        ctx.set_engine("Poppler (pdfimages)");
    } else {
        native_images(ctx, input, &tmp.path, &stem)?;
        ctx.set_engine("Native (lopdf)");
    }
    ctx.progress(0.7);
    let files: Vec<PathBuf> = std::fs::read_dir(&tmp.path)?.filter_map(|e| e.ok().map(|e| e.path())).collect();
    if files.is_empty() {
        return Err(ui!("errors.pdfNoImages"));
    }
    if as_jpg {
        for f in &files {
            ctx.check()?;
            let ext = crate::fsutil::ext_lower(f);
            if ext == "jpg" || ext == "jpeg" {
                continue;
            }
            // Formats we cannot decode (e.g. JBIG2, CCITT) are kept as-is.
            if let Ok(img) = image::open(f) {
                let dst = f.with_extension("jpg");
                if img.to_rgb8().save_with_format(&dst, image::ImageFormat::Jpeg).is_ok() {
                    let _ = std::fs::remove_file(f);
                }
            }
        }
    }
    tmp.commit(out_dir)?;
    Ok(vec![out_dir.to_path_buf()])
}

fn native_images(ctx: &Ctx, input: &Path, dir: &Path, stem: &str) -> R<()> {
    let doc = load(input)?;
    let mut seen = std::collections::HashSet::new();
    let mut n = 0;
    for (_, page_id) in doc.get_pages() {
        ctx.check()?;
        let Ok(images) = doc.get_page_images(page_id) else { continue };
        for img in images {
            if !seen.insert(img.id) {
                continue;
            }
            let filters = img.filters.clone().unwrap_or_default();
            let path_for = |ext: &str| dir.join(format!("{stem}-{n:03}.{ext}"));
            if filters.iter().any(|f| f == "DCTDecode") {
                std::fs::write(path_for("jpg"), img.content)?;
            } else if filters.iter().any(|f| f == "JPXDecode") {
                std::fs::write(path_for("jp2"), img.content)?;
            } else if img.bits_per_component == Some(8) {
                let Ok(stream) = doc.get_object(img.id).and_then(|o| o.as_stream()) else { continue };
                let Ok(data) = stream.decompressed_content() else { continue };
                let (w, h) = (img.width as u32, img.height as u32);
                let saved = match img.color_space.as_deref() {
                    Some("DeviceRGB") => image::RgbImage::from_raw(w, h, data).map(|i| i.save(path_for("png"))),
                    Some("DeviceGray") => image::GrayImage::from_raw(w, h, data).map(|i| i.save(path_for("png"))),
                    _ => None,
                };
                if !matches!(saved, Some(Ok(()))) {
                    continue;
                }
            } else {
                continue;
            }
            n += 1;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_page_pdf(path: &Path, text: &str) {
        use lopdf::content::{Content, Operation};
        use lopdf::{dictionary, Stream};
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" });
        let resources_id = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font_id } });
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 24.into()]),
                Operation::new("Td", vec![100.into(), 600.into()]),
                Operation::new("Tj", vec![Object::string_literal(text)]),
                Operation::new("ET", vec![]),
            ],
        };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
            "Resources" => resources_id, "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        doc.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 }));
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog_id);
        doc.save(path).unwrap();
    }

    #[test]
    fn merge_and_convert() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (d.path().join("a.pdf"), d.path().join("b.pdf"));
        one_page_pdf(&a, "Alpha page");
        one_page_pdf(&b, "Beta page");
        let ctx = Ctx::for_test();
        let out = d.path().join("merged.pdf");
        merge(&ctx, &[a.clone(), b], &out).unwrap();
        assert_eq!(Document::load(&out).unwrap().get_pages().len(), 2);

        if deps::find(Tool::Pdftotext).is_none() {
            let txt = d.path().join("a.txt");
            convert(&ctx, &out, "txt", &txt).unwrap();
            let t = std::fs::read_to_string(&txt).unwrap();
            assert!(t.contains("Alpha") && t.contains("Beta"), "{t}");
            let x = d.path().join("a.xlsx");
            convert(&ctx, &a, "xlsx", &x).unwrap();
            assert!(x.metadata().unwrap().len() > 0);
            let w = d.path().join("a.docx");
            if deps::find(Tool::Soffice).is_none() {
                convert(&ctx, &a, "docx", &w).unwrap();
                assert!(w.metadata().unwrap().len() > 0);
            }
        }
    }

    #[test]
    fn bbox_grid() {
        let w = |x0: f64, x1: f64, y: f64, t: &str| format!("<word xMin=\"{x0}\" yMin=\"{y}\" xMax=\"{x1}\" yMax=\"{}\">{t}</word>", y + 11.0);
        let html = format!(
            "<page width=\"595\">{}{}{}{}{}</page>",
            w(57.0, 90.0, 104.0, "Bergen"),
            w(132.5, 165.9, 104.1, "291940"),
            w(178.0, 194.7, 104.1, "465"),
            w(57.0, 80.0, 120.0, "New"),
            w(83.0, 105.0, 120.0, "York &amp; Co"),
        );
        let g = grids_from_bbox(&html);
        assert_eq!(g[0], vec![vec!["Bergen", "291940", "465"], vec!["New York & Co"]]);
    }

    #[test]
    fn cells() {
        assert_eq!(split_cells("Name   Qty  Price"), vec!["Name", "Qty", "Price"]);
        assert_eq!(split_cells("New York  12"), vec!["New York", "12"]);
    }

    #[test]
    fn html_escapes() {
        let h = text_to_html("a<b", &["x & <y>".into()]);
        assert!(h.contains("a&lt;b"));
        assert!(h.contains("x &amp; &lt;y&gt;"));
    }
}
