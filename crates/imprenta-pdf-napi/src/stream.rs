//! Feeding a document from Node without ever holding it whole.
//!
//! [`crate::job`] takes a document that already exists as a string. This takes
//! it in pieces, so the largest thing the caller holds is one batch of rows
//! rather than the ledger.
//!
//! # Why there is a thread in here
//!
//! A [`Session`] is not `Send`. krilla keeps its fonts behind an `Rc`, so a
//! half-written document cannot move between threads at all — which rules out
//! the obvious design of putting each batch on libuv's pool, because the pool
//! picks a different thread each time.
//!
//! So the session gets a thread of its own and never leaves it. Batches arrive
//! down a channel and promises are resolved back from there. The main thread
//! does no work beyond handing over a string, which is the property that made
//! the engine usable from a server: a service stays answerable while it
//! prints, and measuring a hundred thousand rows inline would take that away
//! along with the memory it saves.
//!
//! Every call is awaited before the next begins — a stream is read in order,
//! and two promises in flight have no order at all. That is not left to the
//! caller to remember: a second call while one is running is refused.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Sender, channel, sync_channel};

use imprenta_pdf::build::Assets;
use imprenta_pdf::ir;
use imprenta_pdf::render::Options;
use imprenta_pdf::session::{Bands, Chunk, Session};
use napi::JsDeferred;
use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::job::{FontInput, ImageInput, JobError, assets_from, face};

/// Resolves a promise on the main thread, from wherever it is called.
type Done<T> = JsDeferred<T, Box<dyn FnOnce(Env) -> Result<T>>>;

/// A piece of a document, still as the caller wrote it.
///
/// Parsed on the worker thread rather than the main one: the main thread's
/// job is to take the string and get out of the way.
enum Piece {
    Nodes(String),
    OpenTable(String),
    Rows(String),
    CloseTable,
}

impl Piece {
    fn parse(self) -> std::result::Result<Chunk, JobError> {
        let bad = |e: serde_json::Error| JobError::Malformed(e.to_string());
        Ok(match self {
            Piece::Nodes(json) => Chunk::Nodes(serde_json::from_str(&json).map_err(bad)?),
            Piece::OpenTable(json) => Chunk::OpenTable(serde_json::from_str(&json).map_err(bad)?),
            Piece::Rows(json) => Chunk::Rows(serde_json::from_str(&json).map_err(bad)?),
            Piece::CloseTable => Chunk::CloseTable,
        })
    }
}

enum Command {
    Feed(Piece, Done<()>),
    Finish(Option<String>, Done<StreamResult>),
}

/// A document being fed to the engine a piece at a time.
#[napi]
pub struct DocumentStream {
    commands: Option<Sender<Command>>,
    /// Set on the main thread when a call starts, cleared when it settles.
    busy: Arc<AtomicBool>,
    /// Atoms the engine is still holding. Read without a round trip, because
    /// this is the number the whole design exists to keep flat and asking for
    /// it should not cost a thread hop.
    pending: Arc<AtomicUsize>,
}

#[napi(object)]
pub struct StreamFont {
    /// `"regular"` or `"bold"`. Defaults to regular.
    pub weight: Option<String>,
    pub italic: Option<bool>,
    pub data: Buffer,
}

#[napi(object)]
pub struct StreamImage {
    pub name: String,
    pub data: Buffer,
}

#[napi(object)]
pub struct StreamOptions {
    pub fonts: Vec<StreamFont>,
    pub images: Option<Vec<StreamImage>>,
    /// Names of the running totals, in the order a band refers to them.
    pub accumulators: Option<Vec<String>>,
    /// A header, as JSON, repeated at the top of every page.
    ///
    /// Given here rather than fed as a chunk because it belongs to the
    /// document: the paginator has to know how much room it takes before it
    /// packs the first row.
    pub header: Option<String>,
    pub footer: Option<String>,
}

#[napi(object)]
pub struct StreamResult {
    /// Present only when no path was given.
    pub pdf: Option<Buffer>,
    pub path: Option<String>,
    pub pages: u32,
    pub bytes: u32,
    pub diagnostics: Vec<String>,
}

#[napi]
impl DocumentStream {
    /// Opens a document. Nothing is measured until something is fed.
    ///
    /// `page` is the same JSON a whole document puts in its `page` field, so
    /// the two ways of printing cannot describe a page differently.
    #[napi(constructor)]
    pub fn new(page: String, options: StreamOptions) -> Result<Self> {
        let setup: ir::PageSetup =
            serde_json::from_str(&page).map_err(|e| fail(JobError::Malformed(e.to_string())))?;

        let mut assets = Assets::new();
        for font in options.fonts {
            let input = FontInput {
                weight: font.weight.unwrap_or_default(),
                italic: font.italic.unwrap_or(false),
                data: font.data.to_vec(),
            };
            assets = assets.with_font(face(&input).map_err(fail)?, input.data);
        }
        let assets = assets_from(
            assets,
            options
                .images
                .unwrap_or_default()
                .into_iter()
                .map(|image| ImageInput {
                    name: image.name,
                    data: image.data.to_vec(),
                }),
        )
        .map_err(fail)?;

        let names = options.accumulators.unwrap_or_default();
        let read = |band: Option<String>| -> Result<Option<imprenta_pdf::ir::Band>> {
            band.map(|json| {
                serde_json::from_str(&json).map_err(|e| fail(JobError::Malformed(e.to_string())))
            })
            .transpose()
        };
        let bands = Bands {
            header: read(options.header)?,
            footer: read(options.footer)?,
        };
        let pending = Arc::new(AtomicUsize::new(0));
        // Unbounded, so handing over a batch never blocks the main thread.
        // Order comes from the channel being first in, first out and the
        // sends happening on the one thread that can make them — not from
        // the caller remembering to await, which `claim` enforces separately
        // so an unawaited loop cannot pile the whole ledger up in here.
        let (commands, orders) = channel::<Command>();

        // The session is built on the thread that will own it. Sending it
        // there is not an option: it is not `Send`, which is the reason this
        // thread exists.
        let (ready, started) = sync_channel::<Result<()>>(1);
        let counter = pending.clone();
        let busy = Arc::new(AtomicBool::new(false));
        let free = busy.clone();
        std::thread::Builder::new()
            .name("imprenta-document".into())
            .spawn(move || {
                let mut session =
                    match Session::open_with(setup, bands, names.len(), assets, Options::default())
                        .map(|s| s.with_accumulator_names(names))
                    {
                        Ok(session) => {
                            let _ = ready.send(Ok(()));
                            session
                        }
                        Err(e) => {
                            let _ = ready.send(Err(fail(JobError::Build(e.to_string()))));
                            return;
                        }
                    };

                for command in orders {
                    match command {
                        Command::Feed(piece, done) => {
                            let outcome = piece.parse().and_then(|chunk| {
                                session
                                    .feed(&chunk)
                                    .map_err(|e| JobError::Build(e.to_string()))
                            });
                            counter.store(session.pending(), Ordering::Relaxed);
                            // Released before settling, and released on
                            // failure too: one bad batch must not wedge the
                            // document shut with the real error buried under
                            // a complaint about being busy. Anything sent in
                            // the gap queues behind this one, so the order
                            // still holds.
                            free.store(false, Ordering::SeqCst);
                            match outcome {
                                Ok(()) => done.resolve(Box::new(|_| Ok(()))),
                                Err(e) => done.reject(fail(e)),
                            }
                        }
                        Command::Finish(path, done) => {
                            free.store(false, Ordering::SeqCst);
                            match finish(session, path) {
                                Ok(result) => done.resolve(Box::new(move |_| Ok(result))),
                                Err(e) => done.reject(e),
                            }
                            // The session is consumed either way; anything
                            // after this would have nothing to work with.
                            return;
                        }
                    }
                }
            })
            .map_err(|e| {
                Error::new(
                    Status::GenericFailure,
                    format!("could not start a thread for the document: {e}"),
                )
            })?;

        started
            .recv()
            .map_err(|_| fail(JobError::Build("the document's thread died".into())))??;

        Ok(Self {
            commands: Some(commands),
            busy,
            pending,
        })
    }

    /// Adds a batch of nodes — headings, paragraphs, whole short tables.
    ///
    /// A batch rather than one at a time because a document with no table in
    /// it is a real document, and forty thousand paragraphs sent singly is
    /// forty thousand round trips: measured, that alone made a transcript a
    /// quarter slower than declaring it whole.
    #[napi(ts_return_type = "Promise<void>")]
    pub fn nodes<'e>(&self, env: &'e Env, nodes: String) -> Result<Object<'e>> {
        self.send(env, Piece::Nodes(nodes))
    }

    /// Begins a table. Its rows follow, in as many batches as suits the caller.
    #[napi(ts_return_type = "Promise<void>")]
    pub fn open_table<'e>(&self, env: &'e Env, head: String) -> Result<Object<'e>> {
        self.send(env, Piece::OpenTable(head))
    }

    /// Adds a batch of rows to the open table.
    #[napi(ts_return_type = "Promise<void>")]
    pub fn rows<'e>(&self, env: &'e Env, rows: String) -> Result<Object<'e>> {
        self.send(env, Piece::Rows(rows))
    }

    #[napi(ts_return_type = "Promise<void>")]
    pub fn close_table<'e>(&self, env: &'e Env) -> Result<Object<'e>> {
        self.send(env, Piece::CloseTable)
    }

    /// Paints what is left and closes the file.
    ///
    /// With a path the PDF is written from Rust and never becomes a Buffer;
    /// without one it comes back, which costs a copy of it in the JS heap.
    #[napi(ts_return_type = "Promise<StreamResult>")]
    pub fn finish<'e>(&mut self, env: &'e Env, path: Option<String>) -> Result<Object<'e>> {
        let (done, promise) = env.create_deferred::<StreamResult, _>()?;
        match self
            .claim()
            .and_then(|()| self.commands.take().ok_or_else(finished))
        {
            Ok(commands) => {
                if commands.send(Command::Finish(path, done)).is_err() {
                    return Err(gone());
                }
            }
            Err(e) => done.reject(e),
        }
        Ok(promise)
    }

    /// Atoms the engine is still holding: about a page's worth, whatever has
    /// been fed. Flat is the whole point, so it is worth being able to look.
    #[napi(getter)]
    pub fn pending(&self) -> u32 {
        self.pending.load(Ordering::Relaxed) as u32
    }

    /// Hands a piece to the document's thread and gives back the promise.
    ///
    /// Every refusal comes back as a rejection rather than a thrown error.
    /// A method the types say returns a promise but which sometimes throws
    /// before making one is a trap: `printer.rows(batch).catch(…)` would not
    /// catch it, and nothing in the signature says so.
    fn send<'e>(&self, env: &'e Env, piece: Piece) -> Result<Object<'e>> {
        let (done, promise) = env.create_deferred::<(), _>()?;
        match self
            .claim()
            .and_then(|()| self.commands.as_ref().ok_or_else(finished))
        {
            Ok(commands) => {
                if commands.send(Command::Feed(piece, done)).is_err() {
                    return Err(gone());
                }
            }
            Err(e) => done.reject(e),
        }
        Ok(promise)
    }

    /// Claims the document for one call, or says who already has it.
    ///
    /// Order does not depend on this — the channel is first in, first out —
    /// but memory does. A loop that forgets to await would queue the whole
    /// ledger here, which is the one thing this API exists to avoid.
    fn claim(&self) -> Result<()> {
        if self.busy.swap(true, Ordering::SeqCst) {
            return Err(Error::new(
                Status::GenericFailure,
                "a document is fed one piece at a time, so each call has to settle before the \
                 next begins: await this one",
            ));
        }
        Ok(())
    }
}

fn finish(session: Session, path: Option<String>) -> Result<StreamResult> {
    let built = session
        .finish()
        .map_err(|e| fail(JobError::Build(e.to_string())))?;
    let bytes = built.pdf.len();
    let pdf = match &path {
        Some(path) => {
            std::fs::write(path, &built.pdf).map_err(|e| {
                fail(JobError::Unwritable {
                    path: path.clone(),
                    reason: e.to_string(),
                })
            })?;
            None
        }
        None => Some(built.pdf.into()),
    };
    Ok(StreamResult {
        pdf,
        path,
        pages: built.pages as u32,
        bytes: bytes as u32,
        diagnostics: built.diagnostics,
    })
}

fn fail(e: JobError) -> Error {
    Error::new(Status::GenericFailure, e.to_string())
}

fn finished() -> Error {
    Error::new(
        Status::GenericFailure,
        "this document has already been finished",
    )
}

fn gone() -> Error {
    Error::new(
        Status::GenericFailure,
        "the document's thread is no longer running",
    )
}
