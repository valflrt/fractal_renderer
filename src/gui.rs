use eframe::{
    egui::{self, Image, Vec2},
    App, CreationContext, Frame,
};

use crate::params::FractalParams;

pub struct Gui {
    fractal_params: Option<FractalParams>,
    preview_bytes: Vec<u8>,
}

impl Gui {
    pub fn new(cc: &CreationContext, preview_bytes: Vec<u8>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        Gui {
            fractal_params: None,
            preview_bytes,
        }
    }
}

impl App for Gui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.columns_const(|[c1, c2]| {
                c1.horizontal(|ui| {
                    ui.label("position: ");
                });

                c2.collapsing("Preview", |ui| {
                    ui.add_sized(
                        Vec2::new(128., 128.),
                        Image::from_bytes(
                            "bytes://fractal_preview.png",
                            self.preview_bytes.to_owned(),
                        ),
                    );
                })
            });
        });
    }
}
