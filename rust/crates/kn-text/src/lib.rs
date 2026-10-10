//! `reader.py` 文本排版的 Rust 移植: HTML -> 块 -> 换行 -> 分页。
//!
//! 模块划分:
//! - [`clean`]: Python 字符串语义 (`py_isspace`/`clean_text`)。
//! - [`html`]: HTML 解析与块抽取 (对应 `sanitize_html`/`extract_blocks`)。
//! - [`wrap`]: 逐行换行与标点禁则 (对应 `_wrap_line_parts`)。
//! - [`paginate`]: 分页与锚点查找 (对应 `ReaderDocument`)。
//! - [`measure`]: 基于 kn-render 字体的 [`wrap::Measure`] 实现。

pub mod clean;
pub mod html;
pub mod measure;
pub mod paginate;
pub mod wrap;

pub use clean::{clean_text, py_isspace};
pub use html::{absolute_url, extract_blocks, Block, BlockKind};
pub use measure::{FontMeasure, WidthCache};
pub use paginate::{
    first_anchor_on_page, first_path_on_page, page_for_path, paginate_all, LayoutParams, Page,
    PageItem, Paginator, PYTHON_IMAGE_MAX_RATIO,
};
pub use wrap::{wrap, Measure};
