// Modified version of https://github.com/TooManyBees/lab

use lab::Lab;

// κ and ε parameters used in conversion between XYZ and La*b*.  See
// http://www.brucelindbloom.com/LContinuity.html for explanation as to why
// those are different values than those provided by CIE standard.
const KAPPA: f32 = 24389.0 / 27.0;
const EPSILON: f32 = 216.0 / 24389.0;

pub fn rgb_to_lab(rgb: &[f32; 3]) -> Lab {
    xyz_to_lab(rgb_to_xyz(rgb))
}

fn rgb_to_xyz(rgb: &[f32; 3]) -> [f32; 3] {
    let r = rgb_to_xyz_map(rgb[0]);
    let g = rgb_to_xyz_map(rgb[1]);
    let b = rgb_to_xyz_map(rgb[2]);

    [
        r * 0.4124564390896921 + g * 0.357576077643909 + b * 0.18043748326639894,
        r * 0.21267285140562248 + g * 0.715152155287818 + b * 0.07217499330655958,
        r * 0.019333895582329317 + g * 0.119192025881303 + b * 0.9503040785363677,
    ]
}

#[inline]
fn rgb_to_xyz_map(c: f32) -> f32 {
    if c > 10. / 255. {
        const A: f32 = 0.055;
        const D: f32 = 1.0 / 1.055;
        ((c as f32 + A) * D).powf(2.4)
    } else {
        const D: f32 = 1.0 / 12.92;
        c as f32 * D
    }
}

fn xyz_to_lab(xyz: [f32; 3]) -> Lab {
    let x = xyz_to_lab_map(xyz[0] * (1.0 / 0.95047));
    let y = xyz_to_lab_map(xyz[1]);
    let z = xyz_to_lab_map(xyz[2] * (1.0 / 1.08883));

    Lab {
        l: (116.0 * y) - 16.0,
        a: 500.0 * (x - y),
        b: 200.0 * (y - z),
    }
}

#[inline]
fn xyz_to_lab_map(c: f32) -> f32 {
    if c > EPSILON {
        c.powf(1.0 / 3.0)
    } else {
        (KAPPA * c + 16.0) * (1.0 / 116.0)
    }
}
