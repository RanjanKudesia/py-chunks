//! Binding generator for the legacy binary formats (`.doc`, `.ppt`).
//!
//! These two shipped a *per-mode* pyfunction surface rather than the
//! `chunk(path, mode, …)` pair [`crate::bind_format`] assumes: 6 chunkers, 6
//! stream entry points each returning its own `#[pyclass]`, 6 image variants,
//! and 2 markdown functions — 20 names and 6 classes per format, all importable
//! from `py_chunks._rust` and therefore all part of the ABI. They are generated
//! rather than written twice, for the reason `bind_format!` exists: the second
//! copy is where the fork grows back (see CONSOLIDATION_PLAN.md).
//!
//! Every name is spelled out at the call site rather than derived from a
//! prefix. These identifiers are the public surface, so `grep` must find them.

/// Per-mode stream iterators. Chunks are converted once at construction; the
/// engine materialises its `stream` anyway, so this is what the fork did too.
#[macro_export]
#[doc(hidden)]
macro_rules! __per_mode_iterators {
    ($($cls:ident),* $(,)?) => {
        $(
            #[pyclass]
            pub struct $cls {
                chunks: std::vec::IntoIter<PyObject>,
            }

            #[pymethods]
            impl $cls {
                fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
                    slf
                }
                fn __next__(&mut self) -> Option<PyObject> {
                    self.chunks.next()
                }
            }

            impl $cls {
                fn build(
                    py: Python<'_>,
                    chunks: &[chunks_rs::chunk::Chunk],
                ) -> PyResult<Self> {
                    Ok($cls {
                        chunks: $crate::engine::chunks_to_pylist(py, chunks)?.into_iter(),
                    })
                }
            }
        )*
    };
}

/// The whole `.doc` / `.ppt` pyfunction surface over a vendored-engine module.
///
/// The engine owns mode dispatch, argument validation and error text; this
/// expansion only names the modes and converts the results. Anything more than
/// that belongs in `rs-chunks`.
#[macro_export]
macro_rules! bind_per_mode_format {
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
        streams = {
            structural   = $s_struct:ident,
            section      = $s_section:ident,
            semantic     = $s_semantic:ident,
            sliding      = $s_slide:ident,
            sentence     = $s_sentence:ident,
            page_aware   = $s_page:ident,
        },
        iterators = {
            structural   = $i_struct:ident,
            section      = $i_section:ident,
            semantic     = $i_semantic:ident,
            sliding      = $i_slide:ident,
            sentence     = $i_sentence:ident,
            page_aware   = $i_page:ident,
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
            let (chunks, images) = __engine::chunk_with_images(
                file_path,
                mode,
                window_size,
                overlap,
                sentences_per_chunk,
                paragraphs_per_page,
            )
            .map_err(to_py_err)?;
            Ok((chunks_to_pylist(py, &chunks)?, images_to_py(py, images)))
        }

        fn __chunks_for(
            file_path: &str,
            mode: &str,
            window_size: usize,
            overlap: usize,
            sentences_per_chunk: usize,
            paragraphs_per_page: usize,
        ) -> PyResult<Vec<chunks_rs::chunk::Chunk>> {
            __engine::chunk(
                file_path,
                mode,
                window_size,
                overlap,
                sentences_per_chunk,
                paragraphs_per_page,
            )
            .map_err(to_py_err)
        }

        $crate::__per_mode_iterators!(
            $i_struct, $i_section, $i_semantic, $i_slide, $i_sentence, $i_page,
        );

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

        // ---- streams --------------------------------------------------------

        #[pyfunction]
        fn $s_struct(py: Python<'_>, file_path: &str) -> PyResult<$i_struct> {
            $i_struct::build(py, &__chunks_for(file_path, "structural", 3, 1, 3, 15)?)
        }
        #[pyfunction]
        fn $s_section(py: Python<'_>, file_path: &str) -> PyResult<$i_section> {
            $i_section::build(py, &__chunks_for(file_path, "section", 3, 1, 3, 15)?)
        }
        #[pyfunction]
        fn $s_semantic(py: Python<'_>, file_path: &str) -> PyResult<$i_semantic> {
            $i_semantic::build(py, &__chunks_for(file_path, "semantic", 3, 1, 3, 15)?)
        }
        #[pyfunction]
        fn $s_slide(
            py: Python<'_>,
            file_path: &str,
            window_size: usize,
            overlap: usize,
        ) -> PyResult<$i_slide> {
            let chunks =
                __chunks_for(file_path, "sliding_window", window_size, overlap, 3, 15)?;
            $i_slide::build(py, &chunks)
        }
        #[pyfunction]
        fn $s_sentence(
            py: Python<'_>,
            file_path: &str,
            sentences_per_chunk: usize,
        ) -> PyResult<$i_sentence> {
            let chunks = __chunks_for(file_path, "sentence", 3, 1, sentences_per_chunk, 15)?;
            $i_sentence::build(py, &chunks)
        }
        #[pyfunction]
        fn $s_page(
            py: Python<'_>,
            file_path: &str,
            paragraphs_per_page: usize,
        ) -> PyResult<$i_page> {
            let chunks = __chunks_for(file_path, "page_aware", 3, 1, 3, paragraphs_per_page)?;
            $i_page::build(py, &chunks)
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
        fn $f_md(file_path: &str) -> PyResult<String> {
            __engine::to_markdown(file_path).map_err(to_py_err)
        }

        #[pyfunction]
        fn $f_mdi(
            py: Python<'_>,
            file_path: &str,
        ) -> PyResult<(String, Vec<(String, Py<PyBytes>)>)> {
            let (md, images) = __engine::to_markdown_with_images(file_path).map_err(to_py_err)?;
            Ok((md, images_to_py(py, images)))
        }

        pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
            m.add_function(wrap_pyfunction!($f_struct, m)?)?;
            m.add_function(wrap_pyfunction!($f_section, m)?)?;
            m.add_function(wrap_pyfunction!($f_semantic, m)?)?;
            m.add_function(wrap_pyfunction!($f_slide, m)?)?;
            m.add_function(wrap_pyfunction!($f_sentence, m)?)?;
            m.add_function(wrap_pyfunction!($f_page, m)?)?;

            m.add_function(wrap_pyfunction!($s_struct, m)?)?;
            m.add_function(wrap_pyfunction!($s_section, m)?)?;
            m.add_function(wrap_pyfunction!($s_semantic, m)?)?;
            m.add_function(wrap_pyfunction!($s_slide, m)?)?;
            m.add_function(wrap_pyfunction!($s_sentence, m)?)?;
            m.add_function(wrap_pyfunction!($s_page, m)?)?;

            m.add_class::<$i_struct>()?;
            m.add_class::<$i_section>()?;
            m.add_class::<$i_semantic>()?;
            m.add_class::<$i_slide>()?;
            m.add_class::<$i_sentence>()?;
            m.add_class::<$i_page>()?;
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
