use image::Rgb;

const DEFAULT_GRADIENT: &[(f64, Rgb<u8>)] = &[
    (0.1, Rgb([2, 0, 4])),
    (0.6, Rgb([80, 60, 100])),
    (1., Rgb([240, 220, 210])),
];

pub fn color_mapping(t: f64) -> Rgb<u8> {
    fn map(t: f64, gradient: &[(f64, Rgb<u8>)]) -> Rgb<u8> {
        let first = gradient[0];
        let last = gradient.last().unwrap();
        if t <= first.0 {
            first.1
        } else if t >= last.0 {
            last.1
        } else {
            for i in 0..gradient.len() {
                if gradient[i].0 <= t && t <= gradient[i + 1].0 {
                    let ratio = (t - gradient[i].0) / (gradient[i + 1].0 - gradient[i].0);
                    let Rgb([r1, g1, b1]) = gradient[i].1;
                    let Rgb([r2, g2, b2]) = gradient[i + 1].1;
                    let r = (r1 as f64 * (1. - ratio) + r2 as f64 * ratio).clamp(0., 255.) as u8;
                    let g = (g1 as f64 * (1. - ratio) + g2 as f64 * ratio).clamp(0., 255.) as u8;
                    let b = (b1 as f64 * (1. - ratio) + b2 as f64 * ratio).clamp(0., 255.) as u8;
                    return Rgb([r, g, b]);
                }
            }
            last.1
        }
    }

    map(t, DEFAULT_GRADIENT)
}
