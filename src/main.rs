mod coloring;
mod complex4;
mod error;
mod fractal;
mod mat;
mod params;
mod progress;
mod rendering;
mod sampling;

use std::{env, fs, time::Instant};

use image::{Rgb, RgbImage};
use uni_path::PathBuf;

use crate::{
    coloring::{
        color_mapping,
        cumulative_histogram::{compute_histogram, cumulate_histogram, get_histogram_value},
        ColoringMode,
    },
    error::{ErrorKind, Result},
    mat::Mat2D,
    params::{DevOptions, FractalParams, RenderStep},
    progress::Progress,
    rendering::RenderingCtx,
    rendering::{render_raw_image, RDR_KERNEL_SIZE},
    sampling::{generate_sampling_points, preview_sampling_points},
};

const CHUNK_SIZE: usize = 512;

struct ViewParams {
    width: f64,
    height: f64,
    x_min: f64,
    y_min: f64,
}

#[derive(Debug, Clone, Copy)]
struct ChunkDimensions {
    v_chunks: usize,
    h_chunks: usize,
    last_v_chunk: usize,
    last_h_chunk: usize,
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    match args.len() {
        3 => {
            let (param_file_path, output_image_path) =
                (PathBuf::from(&args[1]), PathBuf::from(&args[2]));

            let params = ron::from_str::<FractalParams>(
                &fs::read_to_string(param_file_path.as_str())
                    .map_err(ErrorKind::ReadParameterFile)?,
            )
            .map_err(ErrorKind::DecodeParameterFile)?;

            // println!(
            //     "{}",
            //     ron::ser::to_string_pretty(&params, PrettyConfig::default()).unwrap()
            // );

            let FractalParams {
                img_width,
                img_height,
                render,
                max_iter,
                sampling,
                coloring_mode,
                custom_gradient,
                diverging_areas,
                dev_options,
            } = params;

            // sampling

            let sampling_points = generate_sampling_points(sampling.level);
            if let Some(DevOptions {
                save_sampling_pattern: true,
                ..
            }) = dev_options
            {
                preview_sampling_points(&sampling_points)?;
            }

            // Get chunks

            let v_chunks = (img_height as usize).div_euclid(CHUNK_SIZE);
            let h_chunks = (img_width as usize).div_euclid(CHUNK_SIZE);
            let last_v_chunk = (img_height as usize).rem_euclid(CHUNK_SIZE);
            let last_h_chunk = (img_width as usize).rem_euclid(CHUNK_SIZE);
            let chunk_dims = ChunkDimensions {
                v_chunks,
                h_chunks,
                last_v_chunk,
                last_h_chunk,
            };

            // Render

            let start = Instant::now();
            let stdout = std::io::stdout();

            // Compute escape time (number of iterations) for each pixel

            let rendering_ctx = RenderingCtx {
                img_width,
                img_height,
                max_iter,
                sampling,
                sampling_points: &sampling_points,
                chunk_dims,
                diverging_areas: &diverging_areas,
                start,
                stdout: &stdout,
            };

            match render {
                params::Render::Frame {
                    zoom,
                    center_x,
                    center_y,
                    fractal,
                } => {
                    let view_params = setup_view(img_width, img_height, zoom, center_x, center_y);

                    let progress = init_progress(chunk_dims);

                    let raw_image = render_raw_image(fractal, view_params, rendering_ctx, progress);

                    println!();

                    let output_image = color_raw_image(
                        img_width,
                        img_height,
                        raw_image,
                        coloring_mode,
                        custom_gradient.as_ref(),
                        dev_options,
                    );

                    output_image
                        .save(output_image_path.as_str())
                        .map_err(ErrorKind::SaveImage)?;

                    let image_size = fs::metadata(output_image_path.as_str()).unwrap().len();
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
                        if let Some(ext) = output_image_path.extension() {
                            format!("- {} ", ext)
                        } else {
                            "".to_string()
                        }
                    );
                }
                params::Render::Animation {
                    zoom,
                    center_x,
                    center_y,
                    fractal,
                    duration,
                    fps,
                } => {
                    let frame_count = (duration * fps) as usize;

                    println!("frame count: {}", frame_count);
                    println!();

                    for frame_i in 0..frame_count {
                        let t = frame_i as f64 / fps;

                        let zoom = zoom[RenderStep::get_current_step_index(&zoom, t)].get_value(t);
                        let center_x =
                            center_x[RenderStep::get_current_step_index(&center_x, t)].get_value(t);
                        let center_y =
                            center_y[RenderStep::get_current_step_index(&center_y, t)].get_value(t);

                        let view_params =
                            setup_view(img_width, img_height, zoom, center_x, center_y);

                        let progress = init_progress(chunk_dims);

                        let raw_image = render_raw_image(
                            fractal.get_fractal(t),
                            view_params,
                            rendering_ctx,
                            progress,
                        );

                        println!();

                        let output_image = color_raw_image(
                            img_width,
                            img_height,
                            raw_image,
                            coloring_mode,
                            custom_gradient.as_ref(),
                            dev_options,
                        );

                        let output_image_path = PathBuf::from(
                            output_image_path.parent().unwrap().to_string()
                                + "/"
                                + output_image_path.file_stem().unwrap()
                                + "_"
                                + &format!("{:06}", frame_i)
                                + "."
                                + output_image_path.extension().unwrap(),
                        );

                        output_image
                            .save(output_image_path.as_str())
                            .map_err(ErrorKind::SaveImage)?;

                        let image_size = fs::metadata(output_image_path.as_str()).unwrap().len();
                        println!(
                            " frame {}: {}x{} - {} {}",
                            frame_i + 1,
                            img_width,
                            img_height,
                            if image_size / 1_000_000 != 0 {
                                format!("{:.1}mb", image_size as f32 / 1_000_000.)
                            } else if image_size / 1_000 != 0 {
                                format!("{:.1}kb", image_size as f32 / 1_000.)
                            } else {
                                format!("{}b", image_size)
                            },
                            if let Some(ext) = output_image_path.extension() {
                                format!("- {} ", ext)
                            } else {
                                "".to_string()
                            }
                        );
                        println!();
                    }

                    println!(
                        "{} frames - {:.1}s elapsed",
                        frame_count,
                        start.elapsed().as_secs_f32()
                    )
                }
            }
        }
        _ => {
            println!("This is a fractal renderer.");
            println!("Usage: fractal_renderer <param file path>.json <output image path>.png");
            println!("More information: https://gh.valflrt.dev/fractal_renderer");
        }
    }

    Ok(())
}

fn setup_view(
    img_width: u32,
    img_height: u32,
    zoom: f64,
    center_x: f64,
    center_y: f64,
) -> ViewParams {
    let aspect_ratio = img_width as f64 / img_height as f64;

    let width = zoom;
    let height = width / aspect_ratio;
    let x_min = center_x - width / 2.;
    // make center_y negative to match complex number representation
    // (in which the imaginary axis is pointing upward)
    let y_min = -center_y - height / 2.;

    ViewParams {
        width,
        height,
        x_min,
        y_min,
    }
}

fn init_progress(
    ChunkDimensions {
        v_chunks,
        h_chunks,
        last_v_chunk,
        last_h_chunk,
    }: ChunkDimensions,
) -> Progress {
    let total = (0..v_chunks + 1)
        .flat_map(|cj| {
            (0..h_chunks + 1).map(move |ci| {
                let chunk_width = if ci == h_chunks {
                    last_h_chunk
                } else {
                    CHUNK_SIZE
                };
                let chunk_height = if cj == v_chunks {
                    last_v_chunk
                } else {
                    CHUNK_SIZE
                };

                (chunk_width + 2 * RDR_KERNEL_SIZE) * (chunk_height + 2 * RDR_KERNEL_SIZE)
            })
        })
        .sum::<usize>();

    Progress::new(total)
}

fn color_raw_image(
    img_width: u32,
    img_height: u32,
    mut raw_image: Mat2D<f64>,
    coloring_mode: ColoringMode,
    custom_gradient: Option<&Vec<(f64, [u8; 3])>>,
    dev_options: Option<DevOptions>,
) -> RgbImage {
    let mut output_image = RgbImage::new(img_width, img_height);

    let max_v = raw_image.vec.iter().copied().fold(0., f64::max);
    let min_v = raw_image.vec.iter().copied().fold(max_v, f64::min);

    match coloring_mode {
        ColoringMode::CumulativeHistogram { map } => {
            raw_image.vec.iter_mut().for_each(|v| *v /= max_v);
            let cumulative_histogram = cumulate_histogram(compute_histogram(&raw_image.vec));
            for j in 0..img_height as usize {
                for i in 0..img_width as usize {
                    let &value = raw_image.get((i, j)).unwrap();

                    let t = map.apply(get_histogram_value(value, &cumulative_histogram));

                    output_image.put_pixel(i as u32, j as u32, color_mapping(t, custom_gradient));
                }
            }
        }
        ColoringMode::MaxNorm { max, map } => {
            let max = max.unwrap_or(max_v);

            for j in 0..img_height as usize {
                for i in 0..img_width as usize {
                    let &value = raw_image.get((i, j)).unwrap();

                    let t = map.apply(value / max);

                    output_image.put_pixel(i as u32, j as u32, color_mapping(t, custom_gradient));
                }
            }
        }
        ColoringMode::MinMaxNorm { min, max, map } => {
            let min = min.unwrap_or(min_v);
            let max = max.unwrap_or(max_v);

            for j in 0..img_height as usize {
                for i in 0..img_width as usize {
                    let &value = raw_image.get((i, j)).unwrap();

                    let t = map.apply((value - min) / (max - min));

                    output_image.put_pixel(i as u32, j as u32, color_mapping(t, custom_gradient));
                }
            }
        }
        ColoringMode::BlackAndWhite => {
            for j in 0..img_height as usize {
                for i in 0..img_width as usize {
                    let &value = raw_image.get((i, j)).unwrap();
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
    };

    if let Some(DevOptions {
        display_gradient: true,
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
                    color_mapping(i as f64 / GRADIENT_WIDTH as f64, custom_gradient),
                );
            }
        }
    }

    output_image
}
