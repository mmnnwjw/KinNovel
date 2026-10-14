//! `reader.py` 文本排版的 Rust 移植: HTML -> 块 -> 换行 -> 分页。
//!
//! 模块划分:
//! - [`clean`]: Python 字符串语义 (`py_isspace`/`clean_text`)。
//! - [`html`]: HTML 解析与块抽取 (对应 `sanitize_html`/`extract_blocks`; 另有网页样式模式 `extract_document`)。
//! - [`style`]: 网页版阅读样式 (read.scss + 浏览器默认样式) 到块/行内样式的对应。
//! - [`wrap`]: 逐行换行与标点禁则 (对应 `_wrap_line_parts`)。
//! - [`paginate`]: 分页与锚点查找 (对应 `ReaderDocument`)。
//! - [`measure`]: 基于 kn-render 字体的 [`wrap::Measure`] 实现。

pub mod clean;
pub mod html;
pub mod measure;
pub mod paginate;
pub mod style;
pub mod wrap;

pub use clean::{clean_text, py_isspace};
pub use html::{absolute_url, extract_blocks, extract_document, Block, BlockKind, Document, Inline, Span, OBJECT_CHAR};
pub use measure::{FontMeasure, WidthCache};
pub use paginate::{
    first_anchor_on_page, first_path_on_page, page_for_path, paginate_all, LayoutParams, Page,
    PageItem, Paginator, RichLine, Run, PYTHON_IMAGE_MAX_RATIO, PYTHON_PARAGRAPH_SPACING, url_size_hint,
};
pub use style::{Align, BlockStyle, InlineStyle};
pub use wrap::{wrap, wrap_widths, Measure};
