//! Renders a run as one self-contained HTML page: each reference photo next
//! to the images generated from it, with a side-by-side viewer for checking
//! that the jewelry was reproduced faithfully.

use studio_core::cost::format_usd;

use crate::pipeline::{ItemRecord, RunManifest, StyleRecord, VariantRecord};

pub fn render(manifest: &RunManifest) -> String {
    let mut html = String::from(HEAD);

    let mut facts = vec![
        format!("Model {}", escape(&manifest.model)),
        format!(
            "Size {}",
            escape(manifest.image_size.as_deref().unwrap_or("model default"))
        ),
        format!("{} saved", manifest.saved()),
    ];
    if manifest.failed() > 0 {
        facts.push(format!("{} failed", manifest.failed()));
    }
    if let Some(cost) = manifest.cost_usd() {
        facts.push(format!("about {}", format_usd(cost)));
    }
    if let Some(note) = &manifest.note {
        facts.push(format!("Note: {}", escape(note)));
    }
    html.push_str(&format!(
        "<header><h1>Jewelry Studio test run, {}</h1><div class=\"meta\">{}</div>\
         <div class=\"meta\">Click a generated image to compare it with the reference. \
         Use the arrow keys to move between images and Esc to close.</div></header>\n<main>\n",
        escape(&manifest.started_at),
        facts.join(" &middot; ")
    ));

    for item in &manifest.items {
        render_item(&mut html, item);
    }

    html.push_str("</main>\n");
    html.push_str(TAIL);
    html
}

fn render_item(html: &mut String, item: &ItemRecord) {
    html.push_str(&format!(
        "<section class=\"item\"><h2>{} <span class=\"tag\">{}</span></h2>\n",
        escape(&item.id),
        escape(&item.category_label)
    ));
    if let Some(error) = &item.error {
        html.push_str(&format!(
            "<p class=\"err\">Skipped: {}</p></section>\n",
            escape(error)
        ));
        return;
    }

    let reference = item.reference.as_deref().unwrap_or_default();
    html.push_str(&format!(
        "<div class=\"row\"><div class=\"group\"><h3>Reference</h3><div class=\"tiles\">\
         <div class=\"tile\"><a href=\"{src}\" target=\"_blank\"><img src=\"{src}\" alt=\"Reference photo\"></a>\
         <div class=\"cap\">{source}</div></div></div></div>\n",
        src = escape(reference),
        source = escape(file_name(&item.source))
    ));
    for style in &item.styles {
        render_style(html, item, style, reference);
    }
    html.push_str("</div></section>\n");
}

fn render_style(html: &mut String, item: &ItemRecord, style: &StyleRecord, reference: &str) {
    html.push_str(&format!(
        "<div class=\"group\"><h3>{} v{} &middot; <a href=\"{}\" target=\"_blank\">prompt</a></h3>\
         <div class=\"tiles\">\n",
        escape(&style.style_name),
        style.style_version,
        escape(&style.prompt_file)
    ));
    for variant in &style.variants {
        html.push_str(&render_variant(item, style, variant, reference));
    }
    html.push_str("</div></div>\n");
}

fn render_variant(
    item: &ItemRecord,
    style: &StyleRecord,
    variant: &VariantRecord,
    reference: &str,
) -> String {
    match &variant.file {
        Some(file) => format!(
            "<div class=\"tile\"><img class=\"out\" src=\"{src}\" data-ref=\"{reference}\" \
             data-caption=\"{caption}\" alt=\"{caption}\">\
             <div class=\"cap\">Option {number} &middot; {seconds:.0}s</div></div>\n",
            src = escape(file),
            reference = escape(reference),
            caption = escape(&format!(
                "{} / {} / option {}",
                item.id, style.style_name, variant.number
            )),
            number = variant.number,
            seconds = variant.seconds
        ),
        None => format!(
            "<div class=\"tile failed\"><div class=\"box\">Option {} failed<br><span>{}</span></div></div>\n",
            variant.number,
            escape(variant.error.as_deref().unwrap_or("no image was returned"))
        ),
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            other => escaped.push(other),
        }
    }
    escaped
}

const HEAD: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Jewelry Studio test run</title>
<style>
  :root { color-scheme: dark; --bg: #1e1e1e; --panel: #2b2b2b; --line: #3c3c3c; --text: #dcdcdc; --dim: #9a9a9a; --accent: #4c9aff; --bad: #ff6b6b; }
  * { box-sizing: border-box; }
  body { margin: 0; background: var(--bg); color: var(--text); font: 14px/1.45 "Segoe UI", system-ui, sans-serif; }
  a { color: var(--accent); }
  header { padding: 14px 24px; background: var(--panel); border-bottom: 1px solid var(--line); position: sticky; top: 0; z-index: 5; }
  h1 { margin: 0 0 4px; font-size: 16px; font-weight: 600; }
  .meta { color: var(--dim); font-size: 13px; }
  main { padding: 0 24px 48px; }
  .item { padding: 20px 0; border-bottom: 1px solid var(--line); }
  h2 { margin: 0 0 12px; font-size: 15px; font-weight: 600; }
  .tag { margin-left: 6px; padding: 2px 8px; border: 1px solid var(--line); border-radius: 10px; color: var(--dim); font-size: 12px; font-weight: 400; }
  .row { display: flex; flex-wrap: wrap; gap: 20px; align-items: flex-start; }
  h3 { margin: 0 0 6px; color: var(--dim); font-size: 12px; font-weight: 600; letter-spacing: .05em; text-transform: uppercase; }
  .tiles { display: flex; flex-wrap: wrap; gap: 8px; }
  .tile { width: 220px; }
  .tile img, .tile .box { display: block; width: 220px; height: 220px; border: 1px solid var(--line); border-radius: 4px; background: #111; }
  .tile img { object-fit: contain; }
  .tile img.out { cursor: zoom-in; }
  .tile img.out:hover { border-color: var(--accent); }
  .tile .box { padding: 12px; overflow: auto; color: var(--bad); font-size: 13px; }
  .tile .box span { color: var(--dim); font-size: 12px; overflow-wrap: anywhere; }
  .cap { margin-top: 4px; color: var(--dim); font-size: 12px; overflow-wrap: anywhere; }
  .err { color: var(--bad); }
  #viewer { position: fixed; inset: 0; z-index: 10; display: none; flex-direction: column; background: #111; }
  #viewer.open { display: flex; }
  #viewer .bar { display: flex; justify-content: space-between; align-items: center; gap: 16px; padding: 10px 16px; border-bottom: 1px solid var(--line); background: var(--panel); }
  #viewer button { padding: 4px 12px; border: 1px solid var(--line); border-radius: 4px; background: #3a3a3a; color: var(--text); font: inherit; cursor: pointer; }
  #viewer .panes { flex: 1; min-height: 0; display: grid; grid-template-columns: 1fr 1fr; gap: 2px; }
  #viewer figure { margin: 0; min-height: 0; display: flex; flex-direction: column; }
  #viewer figcaption { padding: 6px 12px; color: var(--dim); font-size: 12px; letter-spacing: .05em; text-transform: uppercase; }
  #viewer figure img { flex: 1; min-height: 0; width: 100%; object-fit: contain; }
</style>
</head>
<body>
"##;

const TAIL: &str = r##"<div id="viewer">
  <div class="bar"><span id="viewer-caption"></span><button id="viewer-close" type="button">Close</button></div>
  <div class="panes">
    <figure><figcaption>Reference</figcaption><img id="viewer-ref" alt="Reference photo"></figure>
    <figure><figcaption>Generated</figcaption><img id="viewer-out" alt="Generated image"></figure>
  </div>
</div>
<script>
  const outputs = Array.from(document.querySelectorAll('img.out'));
  const viewer = document.getElementById('viewer');
  const caption = document.getElementById('viewer-caption');
  const referenceImage = document.getElementById('viewer-ref');
  const outputImage = document.getElementById('viewer-out');
  let current = 0;

  function show(index) {
    current = (index + outputs.length) % outputs.length;
    const image = outputs[current];
    referenceImage.src = image.dataset.ref;
    outputImage.src = image.src;
    caption.textContent = image.dataset.caption + '  (' + (current + 1) + ' of ' + outputs.length + ')';
    viewer.classList.add('open');
  }

  function hide() {
    viewer.classList.remove('open');
  }

  outputs.forEach((image, index) => image.addEventListener('click', () => show(index)));
  document.getElementById('viewer-close').addEventListener('click', hide);
  document.addEventListener('keydown', (event) => {
    if (!viewer.classList.contains('open')) return;
    if (event.key === 'Escape') hide();
    if (event.key === 'ArrowRight') show(current + 1);
    if (event.key === 'ArrowLeft') show(current - 1);
  });
</script>
</body>
</html>
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_escaped_for_html() {
        assert_eq!(
            escape("<b>\"Tom & Jerry's\"</b>"),
            "&lt;b&gt;&quot;Tom &amp; Jerry&#39;s&quot;&lt;/b&gt;"
        );
    }

    #[test]
    fn file_name_handles_both_separators() {
        assert_eq!(file_name(r"samples\ring\a.jpg"), "a.jpg");
        assert_eq!(file_name("samples/ring/a.jpg"), "a.jpg");
        assert_eq!(file_name("a.jpg"), "a.jpg");
    }
}
