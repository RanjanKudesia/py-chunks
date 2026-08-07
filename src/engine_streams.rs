//! Streaming halves of the per-mode binding (see `engine_per_mode.rs`).
//!
//! Both macros expand in the same module as [`crate::bind_per_mode_core`] and
//! use the `__chunks_for` helper it emits. Each emits `register_streams`, so a
//! format pairs core with exactly one of them.
//!
//! Neither is lazy, and neither was before: `.doc`/`.ppt`/`.docx` parse the
//! whole document up front and `.pptx` must read the entire ZIP before any
//! slide is available, so every stream entry point has always been a
//! batch-drain. The engine's `stream` materialises too. Genuine laziness exists
//! in exactly one place in the project — xlsx's `RowStreamState` — and
//! CONSOLIDATION_PLAN.md decides that moves *into* the engine rather than
//! staying in a binding.

/// Per-mode stream iterators. Chunks are converted once at construction.
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

/// Six per-mode stream entry points, each returning its own iterator class
/// (`.doc`, `.ppt`, `.docx`).
#[macro_export]
macro_rules! bind_per_mode_streams {
    (
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
    ) => {
        $crate::__per_mode_iterators!(
            $i_struct, $i_section, $i_semantic, $i_slide, $i_sentence, $i_page,
        );

        #[pyfunction]
        fn $s_struct(py: Python<'_>, file_path: &str) -> PyResult<$i_struct> {
            $i_struct::build(py, &__chunks_for(py, file_path, "structural", 3, 1, 3, 15)?)
        }
        #[pyfunction]
        fn $s_section(py: Python<'_>, file_path: &str) -> PyResult<$i_section> {
            $i_section::build(py, &__chunks_for(py, file_path, "section", 3, 1, 3, 15)?)
        }
        #[pyfunction]
        fn $s_semantic(py: Python<'_>, file_path: &str) -> PyResult<$i_semantic> {
            $i_semantic::build(py, &__chunks_for(py, file_path, "semantic", 3, 1, 3, 15)?)
        }
        #[pyfunction]
        fn $s_slide(
            py: Python<'_>,
            file_path: &str,
            window_size: usize,
            overlap: usize,
        ) -> PyResult<$i_slide> {
            let chunks =
                __chunks_for(py, file_path, "sliding_window", window_size, overlap, 3, 15)?;
            $i_slide::build(py, &chunks)
        }
        #[pyfunction]
        fn $s_sentence(
            py: Python<'_>,
            file_path: &str,
            sentences_per_chunk: usize,
        ) -> PyResult<$i_sentence> {
            let chunks = __chunks_for(py, file_path, "sentence", 3, 1, sentences_per_chunk, 15)?;
            $i_sentence::build(py, &chunks)
        }
        #[pyfunction]
        fn $s_page(
            py: Python<'_>,
            file_path: &str,
            paragraphs_per_page: usize,
        ) -> PyResult<$i_page> {
            let chunks =
                __chunks_for(py, file_path, "page_aware", 3, 1, 3, paragraphs_per_page)?;
            $i_page::build(py, &chunks)
        }

        pub fn register_streams(m: &Bound<'_, PyModule>) -> PyResult<()> {
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
    };
}

/// One generic `stream(path, mode, …)` entry point returning a single iterator
/// class (`.pptx`).
///
/// The engine owns mode dispatch and argument validation here, exactly as it
/// does for the batch chunkers — which is the point of the migration: `.pptx`
/// previously validated the same arguments twice, in two different wordings.
#[macro_export]
macro_rules! bind_single_stream {
    (
        stream   = $s_fn:ident,
        iterator = $i_cls:ident,
    ) => {
        $crate::__per_mode_iterators!($i_cls,);

        #[pyfunction]
        #[pyo3(signature = (file_path, mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page))]
        fn $s_fn(
            py: Python<'_>,
            file_path: &str,
            mode: &str,
            window_size: usize,
            overlap: usize,
            sentences_per_chunk: usize,
            paragraphs_per_page: usize,
        ) -> PyResult<$i_cls> {
            let chunks = __chunks_for(
                py,
                file_path,
                mode,
                window_size,
                overlap,
                sentences_per_chunk,
                paragraphs_per_page,
            )?;
            $i_cls::build(py, &chunks)
        }

        pub fn register_streams(m: &Bound<'_, PyModule>) -> PyResult<()> {
            m.add_function(wrap_pyfunction!($s_fn, m)?)?;
            m.add_class::<$i_cls>()?;
            Ok(())
        }
    };
}
