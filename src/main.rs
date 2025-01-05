mod coloring;
mod error;
mod fractal;
mod mat;

use std::{
    env,
    fs::{self, File},
    io::Write,
    path::PathBuf,
    sync::{atomic, mpsc},
    time::Instant,
};

use image::RgbImage;
use mat::Mat2D;
use num_complex::Complex;
use rayon::iter::{ParallelBridge, ParallelIterator};
use serde::{Deserialize, Serialize};

use coloring::color_mapping;
use error::{ErrorKind, Result};
use fractal::Fractal;

#[derive(Debug, Serialize, Deserialize)]
struct FractalParams {
    img_width: u32,
    img_height: u32,

    zoom: f64,
    center_x: f64,
    center_y: f64,

    max_iter: u32,

    fractal: Fractal,
}

#[derive(Debug, Serialize, Deserialize)]
struct DevOptions {
    save_sampling_pattern: Option<bool>,
    display_gradient: Option<bool>,
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    // In case I want to try out how serde_json serializes the
    // param file
    // fs::write("out.json", serde_json::to_string_pretty(&params).unwrap()).unwrap();

    match args.len() {
        3 => {
            let FractalParams {
                img_width,
                img_height,
                zoom,
                center_x,
                center_y,
                max_iter,
                fractal,
            } = serde_json::from_reader::<_, FractalParams>(
                File::open(&args[1]).map_err(ErrorKind::ReadParameterFile)?,
            )
            .map_err(ErrorKind::DecodeParameterFile)?;

            let aspect_ratio = img_width as f64 / img_height as f64;

            let width = zoom;
            let height = width / aspect_ratio;
            let x_min = center_x - width / 2.;
            // make center_y negative to match complex number representation
            // (in which the imaginary axis is pointing upward)
            let y_min = -center_y - height / 2.;

            // init raw image

            let mut raw_image = Mat2D::filled_with(0u32, img_width as usize, img_height as usize);

            // Compute escape time (number of iterations) for each pixel

            const CHUNK_SIZE: usize = 48;
            let (v_chunks, last_v_chunk) = (
                img_height.div_euclid(CHUNK_SIZE as u32),
                img_height.rem_euclid(CHUNK_SIZE as u32),
            );
            let (h_chunks, last_h_chunk) = (
                img_width.div_euclid(CHUNK_SIZE as u32),
                img_width.rem_euclid(CHUNK_SIZE as u32),
            );

            const SAMPLES_F: u32 = 10;

            // Progress related init

            let start = Instant::now();

            let progress = atomic::AtomicU32::new(0);
            let total = (0..v_chunks + 1)
                .flat_map(|cj| {
                    (0..h_chunks + 1).map(move |ci| {
                        let chunk_width = if ci == h_chunks {
                            last_h_chunk
                        } else {
                            CHUNK_SIZE as u32
                        };
                        let chunk_height = if cj == v_chunks {
                            last_v_chunk
                        } else {
                            CHUNK_SIZE as u32
                        };

                        SAMPLES_F * chunk_width * chunk_height
                    })
                })
                .sum::<u32>();

            let stdout = std::io::stdout();

            for cj in 0..v_chunks + 1 {
                for ci in 0..h_chunks + 1 {
                    let chunk_width = if ci == h_chunks {
                        last_h_chunk
                    } else {
                        CHUNK_SIZE as u32
                    };
                    let chunk_height = if cj == v_chunks {
                        last_v_chunk
                    } else {
                        CHUNK_SIZE as u32
                    };

                    // pi and pj are the coordinates of the first pixel of the
                    // chunk (top-left corner pixel)
                    let pi = ci * CHUNK_SIZE as u32;
                    let pj = cj * CHUNK_SIZE as u32;

                    let sample_count = SAMPLES_F * chunk_width * chunk_height;

                    let (tx, rx) = mpsc::channel();
                    (0..sample_count).par_bridge().for_each_with(tx, |s, _| {
                        let x = pi as f64 + fastrand::f64() * chunk_width as f64;
                        let y = pj as f64 + fastrand::f64() * chunk_height as f64;

                        let re = x_min + width * x / img_width as f64;
                        let im = y_min + height * y / img_height as f64;

                        let (_, values) = fractal.get_pixel(Complex::new(re, im), max_iter);

                        for v in values {
                            let (re, im) = (v.re, v.im);

                            let i = (re - x_min) * (img_width as f64) / width - 0.5;
                            let j = (im - y_min) * (img_height as f64) / height - 0.5;

                            if 0. < i
                                && i < (img_width - 1) as f64
                                && 0. < j
                                && j < (img_height - 1) as f64
                            {
                                s.send((i as u32, j as u32)).unwrap();
                            }
                        }

                        // Using atomic::Ordering::Relaxed because we don't really
                        // care about the order `progress` is updated. As long as it
                        // is updated it should be fine :>
                        progress.fetch_add(1, atomic::Ordering::Relaxed);
                        let progress = progress.load(atomic::Ordering::Relaxed);

                        if progress % (total / 100000 + 1) == 0 {
                            stdout
                                .lock()
                                .write_all(
                                    format!(
                                        "\r {:.1}% - {:.1}s elapsed",
                                        100. * progress as f32 / total as f32,
                                        start.elapsed().as_secs_f32(),
                                    )
                                    .as_bytes(),
                                )
                                .unwrap();
                        }
                    });

                    for (i, j) in rx {
                        let (i, j) = (i as usize, j as usize);

                        raw_image
                            .set((i, j), *raw_image.get((i, j)).unwrap() + 1)
                            .unwrap();
                    }
                }
            }

            let mut output_image = RgbImage::new(img_width, img_height);

            let max = raw_image.vec.iter().copied().fold(0, u32::max);
            let min = raw_image.vec.iter().copied().fold(0, u32::min);

            for j in 0..img_height {
                for i in 0..img_width {
                    let v = *raw_image.get((i as usize, j as usize)).unwrap();
                    let t = (v - min) as f64 / (max - min) as f64;

                    output_image.put_pixel(i, j, color_mapping(t.powf(0.3)));
                }
            }

            println!();

            // for j in 0..img_height as usize {
            //     for i in 0..img_width as usize {
            //         let &value = normalized_image.get((i, j)).unwrap();
            //         output_image.put_pixel(i as u32, j as u32, color_mapping(value.powf(0.4)));
            //     }
            // }

            let path = PathBuf::from(&args[2]);

            output_image.save(&path).map_err(ErrorKind::SaveImage)?;

            let image_size = fs::metadata(&path).unwrap().len();
            println!(
                " output image: {}x{} - {} {}",
                img_width,
                img_height,
                if image_size / 1_000_000 != 0 {
                    format!("{:.1}mb", image_size as f32 / 1_000_000.)
                } else if image_size / 1_000 != 0 {
                    format!("{:.1}kb", image_size as f32 / 1_000.)
                } else {
                    format!("{}b", image_size)
                },
                if let Some(ext) = path.extension() {
                    format!("- {} ", ext.to_str().unwrap())
                } else {
                    "".to_string()
                }
            );
        }
        _ => {
            println!("This is a fractal renderer.");
            println!("Usage: fractal_renderer <param file path>.json <output image path>.png");
            println!("More information: https://gh.valflrt.dev/fractal_renderer");
        }
    }

    Ok(())
}
