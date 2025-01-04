mod coloring;
mod error;
mod fractal;
mod mat;
mod sampling;

use std::{
    env,
    fs::{self, File},
    io::Write,
    path::PathBuf,
    sync::{atomic, mpsc},
    time::Instant,
};

use image::{Rgb, RgbImage};
use mat::Mat2D;
use num_complex::Complex;
use rayon::iter::{ParallelBridge, ParallelIterator};
use sampling::{generate_sampling_points, preview_sampling_points, SamplingLevel};
use serde::{Deserialize, Serialize};

use coloring::{
    color_mapping, compute_histogram, cumulate_histogram, get_histogram_value, ColoringMode,
};
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

    coloring_mode: Option<ColoringMode>,
    sampling: Option<SamplingLevel>,

    custom_gradient: Option<Vec<(f64, [u8; 3])>>,

    dev_options: Option<DevOptions>,
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
                sampling: sampling_mode,
                fractal,
                coloring_mode,
                custom_gradient,
                dev_options,
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

            // sampling

            let sampling_points = generate_sampling_points(sampling_mode);
            if let Some(DevOptions {
                save_sampling_pattern: Some(true),
                ..
            }) = dev_options
            {
                preview_sampling_points(&sampling_points)?;
            }

            let mut raw_image = Mat2D::filled_with(0u32, img_width as usize, img_height as usize);

            // Progress related init

            let start = Instant::now();

            let progress = atomic::AtomicU32::new(0);
            let total = img_width * img_height;

            let stdout = std::io::stdout();

            // Compute escape time (number of iterations) for each pixel

            let (tx, rx) = mpsc::channel();

            (0..img_width)
                .flat_map(|j| (0..img_height).map(move |i| (i, j)))
                .par_bridge()
                .for_each_with(tx, |s, (i, j)| {
                    let x = i as f64 + 0.5;
                    let y = j as f64 + 0.5;

                    sampling_points.iter().for_each(|&(dx, dy)| {
                        let re = x_min + width * (x + dx) / img_width as f64;
                        let im = y_min + height * (y + dy) / img_height as f64;

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
                    });

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

            let mut normalized_image =
                Mat2D::filled_with(0., img_width as usize, img_height as usize);

            let max = raw_image.vec.iter().copied().fold(0, u32::max);
            let min = raw_image.vec.iter().copied().fold(0, u32::min);

            for j in 0..img_height {
                for i in 0..img_width {
                    let (i, j) = (i as usize, j as usize);
                    normalized_image
                        .set(
                            (i, j),
                            (*raw_image.get((i, j)).unwrap() - min) as f64 / (max - min) as f64,
                        )
                        .unwrap();
                }
            }

            println!();

            let mut output_image = RgbImage::new(img_width, img_height);

            match coloring_mode.unwrap_or_default() {
                ColoringMode::BlackAndWhite => {
                    for j in 0..img_height as usize {
                        for i in 0..img_width as usize {
                            let &value = normalized_image.get((i, j)).unwrap();
                            output_image.put_pixel(
                                i as u32,
                                j as u32,
                                if value >= 0.95 {
                                    Rgb([0, 0, 0])
                                } else {
                                    Rgb([255, 255, 255])
                                },
                            );
                        }
                    }
                }
                ColoringMode::Linear => {
                    for j in 0..img_height as usize {
                        for i in 0..img_width as usize {
                            let &value = normalized_image.get((i, j)).unwrap();
                            output_image.put_pixel(
                                i as u32,
                                j as u32,
                                color_mapping(value, custom_gradient.as_ref()),
                            );
                        }
                    }
                }
                ColoringMode::Squared => {
                    for j in 0..img_height as usize {
                        for i in 0..img_width as usize {
                            let &value = normalized_image.get((i, j)).unwrap();
                            output_image.put_pixel(
                                i as u32,
                                j as u32,
                                color_mapping(value.powi(2), custom_gradient.as_ref()),
                            );
                        }
                    }
                }
                ColoringMode::CumulativeHistogram => {
                    let cumulative_histogram =
                        cumulate_histogram(compute_histogram(&normalized_image.vec));
                    for j in 0..img_height as usize {
                        for i in 0..img_width as usize {
                            let &value = normalized_image.get((i, j)).unwrap();
                            output_image.put_pixel(
                                i as u32,
                                j as u32,
                                color_mapping(
                                    get_histogram_value(value, &cumulative_histogram).powi(12),
                                    custom_gradient.as_ref(),
                                ),
                            );
                        }
                    }
                }
            };

            if let Some(DevOptions {
                display_gradient: Some(true),
                ..
            }) = dev_options
            {
                const GRADIENT_HEIGHT: u32 = 8;
                const GRADIENT_WIDTH: u32 = 64;
                const OFFSET: u32 = 8;

                for j in 0..GRADIENT_HEIGHT {
                    for i in 0..GRADIENT_WIDTH {
                        output_image.put_pixel(
                            img_width - GRADIENT_WIDTH - OFFSET + i,
                            img_height - GRADIENT_HEIGHT - OFFSET + j,
                            color_mapping(
                                i as f64 / GRADIENT_WIDTH as f64,
                                custom_gradient.as_ref(),
                            ),
                        );
                    }
                }
            }

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
