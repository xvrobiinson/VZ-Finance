use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{error_banner, field, field_row, footer, grow_field, value};
use crate::app::{Route, VzApp};
use crate::database::repositories::clients::{Client, ClientInput};
use crate::i18n::t;
use crate::services::picture::{Photo, PhotoChange};
use crate::tr;
use crate::ui::components::icon;
use crate::ui::kit::Button;
use crate::ui::theme::{RADIUS_CONTROL, Typeset as _, palette, space, text};

/// Side of the photo preview in the form.
const PREVIEW: f32 = 72.0;

/// Called with the new client's id after the form adds one, instead of
/// opening the client's page (the loan form selects them).
pub type OnCreated = Rc<dyn Fn(i64, &mut Window, &mut App)>;

pub struct ClientForm {
    app: WeakEntity<VzApp>,
    editing: Option<i64>,
    pub name: Entity<InputState>,
    phone: Entity<InputState>,
    email: Entity<InputState>,
    id_number: Entity<InputState>,
    address: Entity<InputState>,
    notes: Entity<TextareaState>,
    /// What saving does to the photo.
    photo: PhotoChange,
    /// The photo as it will be saved, drawn in the form.
    preview: Option<Arc<Image>>,
    /// A picture is being read and prepared.
    reading_photo: bool,
    photo_error: Option<String>,
    on_created: Option<OnCreated>,
    /// Saved once; repeated clicks or Enter presses do nothing more.
    saved: bool,
    error: Option<String>,
    show_errors: bool,
    _subs: Vec<Subscription>,
}

fn jpeg_image(bytes: Vec<u8>) -> Arc<Image> {
    Arc::new(Image::from_bytes(ImageFormat::Jpeg, bytes))
}

impl ClientForm {
    /// `photo` is the client's stored photo when editing one that has it.
    pub fn new(
        app: WeakEntity<VzApp>,
        existing: Option<Client>,
        photo: Option<Vec<u8>>,
        on_created: Option<OnCreated>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let e = existing.as_ref();
        let optional = |v: Option<String>, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(t("Optional", "Opcional")).default_value(v.unwrap_or_default()))
        };
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t("e.g. Marta Quiroga", "ej. Marta Quiroga"))
                .default_value(e.map(|c| c.full_name.clone()).unwrap_or_default())
        });
        let phone = optional(e.and_then(|c| c.phone.clone()), window, cx);
        let email = optional(e.and_then(|c| c.email.clone()), window, cx);
        let id_number = optional(e.and_then(|c| c.id_number.clone()), window, cx);
        let address = optional(e.and_then(|c| c.address.clone()), window, cx);
        let notes = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(3)
                .placeholder(t(
                    "Anything worth remembering: referral, collateral, preferred contact time",
                    "Lo que valga la pena recordar: referencia, garantía, horario de contacto",
                ))
                .default_value(e.map(|c| c.notes.clone()).unwrap_or_default())
        });
        let mut subs = Vec::new();
        for input in [&name, &phone, &email, &id_number, &address] {
            subs.push(cx.subscribe_in(input, window, |_, _, ev: &InputEvent, _, cx| {
                if let InputEvent::Change = ev {
                    cx.notify()
                }
            }));
        }
        Self {
            app,
            editing: e.map(|c| c.id),
            name,
            phone,
            email,
            id_number,
            address,
            notes,
            photo: PhotoChange::Keep,
            preview: photo.map(jpeg_image),
            reading_photo: false,
            photo_error: None,
            on_created,
            saved: false,
            error: None,
            show_errors: false,
            _subs: subs,
        }
    }

    fn input(&self, cx: &App) -> ClientInput {
        ClientInput {
            full_name: value(&self.name, cx),
            phone: value(&self.phone, cx),
            email: value(&self.email, cx),
            id_number: value(&self.id_number, cx),
            address: value(&self.address, cx),
            notes: self.notes.read(cx).value().to_string(),
        }
    }

    /// Opens the Windows Open window to pick the photo.
    fn choose_photo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let started = self.app.update(cx, |app, cx| app.begin_file_dialog("client photo", window, cx)).unwrap_or(false);
        if !started {
            return;
        }
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t("Use this photo", "Usar esta foto").into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = rx.await;
            let _ = this.update_in(cx, |form, window, cx| {
                let picked = form.app.update(cx, |app, _| app.end_file_dialog("client photo", result));
                match picked {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            form.use_photo_file(path, window, cx);
                        }
                    }
                    Ok(Err(why)) => {
                        form.photo_error = Some(tr!(
                            "The Open window couldn't open ({why}). Drag the picture onto the photo box instead.",
                            "La ventana Abrir no se pudo abrir ({why}). Arrastra la imagen al cuadro de la foto."
                        ));
                        cx.notify();
                    }
                    _ => {}
                }
            });
        })
        .detach();
    }

    /// Reads and prepares a picture away from the window, then shows it.
    /// Nothing is saved until the form is.
    pub fn use_photo_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.reading_photo = true;
        self.photo_error = None;
        cx.notify();
        let task = cx.background_spawn(async move {
            let bytes = std::fs::read(&path).map_err(|e| {
                let why = e.kind().to_string();
                tr!(
                    "Couldn't open the file ({why}). Check that it still exists.",
                    "No se pudo abrir el archivo ({why}). Revisa que todavía exista."
                )
            })?;
            Photo::from_upload(&bytes).map_err(|e| e.message().to_string())
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |form, cx| {
                form.reading_photo = false;
                match result {
                    Ok(photo) => {
                        form.preview = Some(jpeg_image(photo.jpeg().to_vec()));
                        form.photo = PhotoChange::Set(photo);
                    }
                    Err(msg) => form.photo_error = Some(msg),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn remove_photo(&mut self, cx: &mut Context<Self>) {
        self.photo = if self.editing.is_some() { PhotoChange::Remove } else { PhotoChange::Keep };
        self.preview = None;
        self.photo_error = None;
        cx.notify();
    }

    /// Validates and saves; closes the dialog only on success. Bound to the
    /// dialog's Confirm action (Enter) and to the primary button.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saved || self.reading_photo {
            return;
        }
        self.show_errors = true;
        let input = self.input(cx);
        if input.full_name.trim().is_empty() {
            cx.notify();
            return;
        }
        let editing = self.editing;
        let photo = self.photo.clone();
        let result = self.app.update(cx, |app, cx| match app.store.save_client(editing, &input, &photo) {
            Ok(id) => {
                app.reload(cx);
                Ok(id)
            }
            Err(e) => Err(app.error_message(&e)),
        });
        match result {
            Ok(Ok(id)) => {
                self.saved = true;
                window.close_dialog(cx);
                let name = input.full_name.trim().to_string();
                let on_created = self.on_created.clone().filter(|_| editing.is_none());
                let _ = self.app.update(cx, |app, cx| {
                    app.notify_success(
                        if editing.is_some() { tr!("Saved {name}", "{name} guardado") } else { tr!("Added {name}", "{name} agregado") },
                        window,
                        cx,
                    );
                    if editing.is_none() && on_created.is_none() {
                        app.navigate(Route::Client(id), window, cx);
                    }
                });
                if let Some(f) = on_created {
                    f(id, window, cx);
                }
            }
            Ok(Err(msg)) => {
                self.error = Some(msg);
                cx.notify();
            }
            Err(_) => window.close_dialog(cx),
        }
    }

    fn photo_field(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let accent = p.accent;
        let has = self.preview.is_some();
        let drop = h_flex()
            .id("photo-drop")
            .size(px(PREVIEW))
            .flex_none()
            .justify_center()
            .rounded(px(RADIUS_CONTROL))
            .border_1()
            .border_color(p.border)
            .bg(p.raised)
            .overflow_hidden()
            .cursor_pointer()
            .drag_over::<ExternalPaths>(move |style, _, _, _| style.border_color(accent).border_2())
            .on_drop(cx.listener(|this, paths: &ExternalPaths, w, cx| {
                if let Some(path) = paths.paths().first() {
                    this.use_photo_file(path.clone(), w, cx);
                }
            }))
            .on_click(cx.listener(|this, _, w, cx| this.choose_photo(w, cx)))
            .child(match &self.preview {
                Some(image) => img(image.clone()).size(px(PREVIEW)).object_fit(ObjectFit::Cover).into_any_element(),
                None => icon(IconName::ImagePlus, p.faint, 20.).into_any_element(),
            });
        let buttons = h_flex()
            .gap(px(space::S))
            .child(
                Button::new("photo-pick")
                    .small()
                    .icon(IconName::Upload)
                    .label(if has { t("Replace…", "Cambiar…") } else { t("Add photo…", "Agregar foto…") })
                    .loading(self.reading_photo)
                    .on_click(cx.listener(|this, _, w, cx| this.choose_photo(w, cx))),
            )
            .when(has, |d| {
                d.child(
                    Button::new("photo-remove")
                        .small()
                        .ghost()
                        .label(t("Remove", "Quitar"))
                        .on_click(cx.listener(|this, _, _, cx| this.remove_photo(cx))),
                )
            });
        let hint = div().typeset(text::CAPTION).text_color(p.muted).child(t(
            "Optional. A photo of the client or their ID. It stays private on this computer and is never printed on receipts. PNG, JPG, GIF, BMP or WebP, up to 20 MB.",
            "Opcional. Una foto del cliente o de su identificación. Queda privada en esta computadora y nunca se imprime en los recibos. PNG, JPG, GIF, BMP o WebP, hasta 20 MB.",
        ));
        let control = h_flex()
            .gap(px(space::M))
            .items_start()
            .child(drop)
            .child(v_flex().flex_1().min_w_0().gap(px(space::S)).child(buttons).child(hint));
        field(&p, t("Photo or ID document", "Foto o documento de identidad"), control, None, self.photo_error.clone())
    }
}

impl Render for ClientForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let input = self.input(cx);
        let name_error = (self.show_errors && input.full_name.trim().is_empty())
            .then(|| t("Enter the client's full name", "Escribe el nombre completo del cliente").to_string());
        let email = input.email.trim();
        let email_error = (!email.is_empty() && self.show_errors && !email.contains('@'))
            .then(|| t("Enter a valid email address", "Escribe un correo válido").to_string());
        v_flex()
            .gap(px(space::L))
            .children(error_banner(&p, &self.error))
            .child(field(&p, t("Full name", "Nombre completo"), Input::new(&self.name), None, name_error))
            .child(
                field_row()
                    .child(grow_field(field(&p, t("ID number", "Número de identificación"), Input::new(&self.id_number), None, None)))
                    .child(grow_field(field(&p, t("Phone", "Teléfono"), Input::new(&self.phone), None, None))),
            )
            .child(
                field_row()
                    .child(grow_field(field(&p, t("Address", "Dirección"), Input::new(&self.address), None, None)))
                    .child(grow_field(field(&p, t("Email", "Correo"), Input::new(&self.email), None, email_error))),
            )
            .child(self.photo_field(cx))
            .child(field(&p, t("Notes", "Notas"), Textarea::new(&self.notes), None, None))
            .child(footer(
                &p,
                if self.editing.is_some() { t("Save changes", "Guardar cambios") } else { t("Add client", "Agregar cliente") },
                !self.reading_photo,
                self.saved,
                cx.listener(|this, _, window, cx| this.submit(window, cx)),
            ))
    }
}
