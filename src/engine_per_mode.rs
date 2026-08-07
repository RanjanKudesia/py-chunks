//! Binding generator for formats with a *per-mode* pyfunction surface.
//!
//! `.doc`, `.ppt`, `.docx` and `.pptx` all shipped one pyfunction per chunking
//! mode rather than the `chunk(path, mode, …)` pair [`crate::bind_format`]
//! assumes — 6 chunkers, 6 image variants and 2 markdown functions each, plus a
//! streaming surface that differs per format (see `engine_streams.rs`). Every
//! one of those names is importable from `py_chunks._rust` and therefore part
//! of the ABI. They are generated rather than written four times, for the
//! reason `bind_format!` exists: the second copy is where the fork grows back
//! (see CONSOLIDATION_PLAN.md).
//!
//! Every name is spelled out at the call site rather than derived from a
//! prefix. These identifiers are the public surface, so `grep` must find them.
//!
//! Split into core (this file) and streams because the streaming shape is the
//! one thing the four formats disagree on: `.doc`/`.ppt`/`.docx` expose six
//! per-mode entry points returning six distinct iterator classes, `.pptx` a
//! single generic `stream_pptx_chunks`. Both stream macros expand alongside
//! [`bind_per_mode_core!`] and use the `__engine` alias and `__chunks_for`
//! helper it brings into scope.

/// The mode-dispatching half: chunkers, image variants, markdown.
///
/// Emits `register_chunkers`, `register_images` and `register_markdown`, plus
/// the `__engine` / `__run_mode` / `__run_images` / `__chunks_for` items the
/// stream macros build on.
#[macro_export]
macro_rules! bind_per_mode_core {
    (
        engine = $eng:path,
        chunkers = {
            structural   = $f_struct:ident,
            section      = $f_section:ident,
            semantic     = $f_semantic:ident,
            sliding      = $f_slide:ident,
            sentence     = $f_sentence:ident,
            page_aware   = $f_page:ident,
        },
        images = {
            structural   = $g_struct:ident,
            section      = $g_section:ident,
            semantic     = $g_semantic:ident,
            sliding      = $g_slide:ident,
            sentence     = $g_sentence:ident,
            page_aware   = $g_page:ident,
        },
        to_markdown             = $f_md:ident,
        to_markdown_with_images = $f_mdi:ident,
    ) => {
        use pyo3::prelude::*;
        use pyo3::types::{PyBytes, PyModule};
        use pyo3::wrap_pyfunction;

        use $eng as __engine;
        use $crate::engine::{chunks_to_pylist, images_to_py, run, to_py_err};

        type __ImagePair = (Vec<PyObject>, Vec<(String, Py<PyBytes>)>);

        /// `window_size` / `overlap` / `sentences_per_chunk` /
        /// `paragraphs_per_page` are passed for every mode; the engine reads
        /// only the ones its mode uses. The values here are the historical
        /// Python defaults and are part of the public API.
        fn __run_mode(
            py: Python<'_>,
            file_path: &str,
            mode: &str,
            window_size: usize,
            overlap: usize,
            sentences_per_chunk: usize,
            paragraphs_per_page: usize,
        ) -> PyResult<PyObject> {
            run(py, || {
                __engine::chunk(
                    file_path,
                    mode,
                    window_size,
                    overlap,
                    sentences_per_chunk,
                    paragraphs_per_page,
                )
            })
        }

        fn __run_images(
            py: Python<'_>,
            file_path: &str,
            mode: &str,
            window_size: usize,
            overlap: usize,
            sentences_per_chunk: usize,
            paragraphs_per_page: usize,
        ) -> PyResult<__ImagePair> {
            // Engine work runs with the GIL released; Python conversion after.
            let (chunks, images) = py
                .allow_threads(|| {
                    __engine::chunk_with_images(
                        file_path,
                        mode,
                        window_size,
                        overlap,
                        sentences_per_chunk,
                        paragraphs_per_page,
                    )
                })
                .map_err(to_py_err)?;
            Ok((chunks_to_pylist(py, &chunks)?, images_to_py(py, images)))
        }

        /// Materialised chunks for a stream entry point — used by whichever
        /// stream macro this format pairs with. Parses with the GIL released.
        #[allow(dead_code)]
        fn __chunks_for(
            py: Python<'_>,
            file_path: &str,
            mode: &str,
            window_size: usize,
            overlap: usize,
            sentences_per_chunk: usize,
            paragraphs_per_page: usize,
        ) -> PyResult<Vec<chunks_rs::chunk::Chunk>> {
            py.allow_threads(|| {
                __engine::chunk(
                    file_path,
                    mode,
                    window_size,
                    overlap,
                    sentences_per_chunk,
                    paragraphs_per_page,
                )
            })
            .map_err(to_py_err)
        }

        // ---- chunkers -------------------------------------------------------

        #[pyfunction]
        fn $f_struct(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
            __run_mode(py, file_path, "structural", 3, 1, 3, 15)
        }
        #[pyfunction]
        fn $f_section(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
            __run_mode(py, file_path, "section", 3, 1, 3, 15)
        }
        #[pyfunction]
        fn $f_semantic(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
            __run_mode(py, file_path, "semantic", 3, 1, 3, 15)
        }
        #[pyfunction]
        fn $f_slide(
            py: Python<'_>,
            file_path: &str,
            window_size: usize,
            overlap: usize,
        ) -> PyResult<PyObject> {
            __run_mode(py, file_path, "sliding_window", window_size, overlap, 3, 15)
        }
        #[pyfunction]
        fn $f_sentence(
            py: Python<'_>,
            file_path: &str,
            sentences_per_chunk: usize,
        ) -> PyResult<PyObject> {
            __run_mode(py, file_path, "sentence", 3, 1, sentences_per_chunk, 15)
        }
        #[pyfunction]
        fn $f_page(
            py: Python<'_>,
            file_path: &str,
            paragraphs_per_page: usize,
        ) -> PyResult<PyObject> {
            __run_mode(py, file_path, "page_aware", 3, 1, 3, paragraphs_per_page)
        }

        // ---- images ---------------------------------------------------------

        #[pyfunction]
        fn $g_struct(py: Python<'_>, file_path: &str) -> PyResult<__ImagePair> {
            __run_images(py, file_path, "structural", 3, 1, 3, 15)
        }
        #[pyfunction]
        fn $g_section(py: Python<'_>, file_path: &str) -> PyResult<__ImagePair> {
            __run_images(py, file_path, "section", 3, 1, 3, 15)
        }
        #[pyfunction]
        fn $g_semantic(py: Python<'_>, file_path: &str) -> PyResult<__ImagePair> {
            __run_images(py, file_path, "semantic", 3, 1, 3, 15)
        }
        #[pyfunction]
        fn $g_slide(
            py: Python<'_>,
            file_path: &str,
            window_size: usize,
            overlap: usize,
        ) -> PyResult<__ImagePair> {
            __run_images(py, file_path, "sliding_window", window_size, overlap, 3, 15)
        }
        #[pyfunction]
        fn $g_sentence(
            py: Python<'_>,
            file_path: &str,
            sentences_per_chunk: usize,
        ) -> PyResult<__ImagePair> {
            __run_images(py, file_path, "sentence", 3, 1, sentences_per_chunk, 15)
        }
        #[pyfunction]
        fn $g_page(
            py: Python<'_>,
            file_path: &str,
            paragraphs_per_page: usize,
        ) -> PyResult<__ImagePair> {
            __run_images(py, file_path, "page_aware", 3, 1, 3, paragraphs_per_page)
        }

        // ---- markdown -------------------------------------------------------

        #[pyfunction]
        fn $f_md(py: Python<'_>, file_path: &str) -> PyResult<String> {
            py.allow_threads(|| __engine::to_markdown(file_path)).map_err(to_py_err)
        }

        #[pyfunction]
        fn $f_mdi(
            py: Python<'_>,
            file_path: &str,
        ) -> PyResult<(String, Vec<(String, Py<PyBytes>)>)> {
            let (md, images) = py
                .allow_threads(|| __engine::to_markdown_with_images(file_path))
                .map_err(to_py_err)?;
            Ok((md, images_to_py(py, images)))
        }

        pub fn register_chunkers(m: &Bound<'_, PyModule>) -> PyResult<()> {
            m.add_function(wrap_pyfunction!($f_struct, m)?)?;
            m.add_function(wrap_pyfunction!($f_section, m)?)?;
            m.add_function(wrap_pyfunction!($f_semantic, m)?)?;
            m.add_function(wrap_pyfunction!($f_slide, m)?)?;
            m.add_function(wrap_pyfunction!($f_sentence, m)?)?;
            m.add_function(wrap_pyfunction!($f_page, m)?)?;
            Ok(())
        }

        pub fn register_images(m: &Bound<'_, PyModule>) -> PyResult<()> {
            m.add_function(wrap_pyfunction!($g_struct, m)?)?;
            m.add_function(wrap_pyfunction!($g_section, m)?)?;
            m.add_function(wrap_pyfunction!($g_semantic, m)?)?;
            m.add_function(wrap_pyfunction!($g_slide, m)?)?;
            m.add_function(wrap_pyfunction!($g_sentence, m)?)?;
            m.add_function(wrap_pyfunction!($g_page, m)?)?;
            Ok(())
        }

        pub fn register_markdown(m: &Bound<'_, PyModule>) -> PyResult<()> {
            m.add_function(wrap_pyfunction!($f_md, m)?)?;
            m.add_function(wrap_pyfunction!($f_mdi, m)?)?;
            Ok(())
        }
    };
}

/// The `.doc` / `.ppt` / `.docx` shape: core plus six per-mode stream entry
/// points, each returning its own iterator class.
///
/// `register` covers chunkers *and* streams, so those formats' `mod.rs` calls
/// `register` / `register_images` / `register_markdown` and nothing else.
#[macro_export]
macro_rules! bind_per_mode_format {
    (
        engine = $eng:path,
        chunkers = { $($chunkers:tt)* },
        streams = { $($streams:tt)* },
        iterators = { $($iterators:tt)* },
        images = { $($images:tt)* },
        to_markdown             = $f_md:ident,
        to_markdown_with_images = $f_mdi:ident,
    ) => {
        $crate::bind_per_mode_core! {
            engine = $eng,
            chunkers = { $($chunkers)* },
            images = { $($images)* },
            to_markdown             = $f_md,
            to_markdown_with_images = $f_mdi,
        }

        $crate::bind_per_mode_streams! {
            streams = { $($streams)* },
            iterators = { $($iterators)* },
        }

        pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
            register_chunkers(m)?;
            register_streams(m)?;
            Ok(())
        }
    };
}
