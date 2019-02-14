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
        pow_2_4((c as f32 + A) * D)
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

fn pow_2_4(x: f32) -> f32 {
    // Closely approximate x^2.4.
    // Divide x by its exponent and a truncated version of itself to get it as close to 1 as
    // possible. Calculate the power of 2.4 using the binomial method. Multiply what was divided to
    // the power of 2.4.

    // Lookup table still have to be hardcoded.
    const FRAC_BITS: u32 = 3;

    // Cast x into an integer to manipulate its exponent and fractional parts into indexes for
    // lookup tables.
    let bits = x.to_bits();

    // Get the integer log2 from the exponent part of bits
    let log2 = (bits >> 23) as i32 - 0x7f;

    // x is always >= (10/255 + A)*D so we only have to deal with a limited range in the exponent.
    // log2 range is [-4, 0]
    // Use a lookup table to offset for dividing by 2^log of x.
    // x^2.4 = (2^log2)^2.4 * (x/(2^log2))^2.4
    let lookup_entry_exp_pow_2_4 =
        |log2: i32| (f32::from_bits(((log2 + 0x7f) << 23) as u32) as f64).powf(2.4) as f32;
    let lookup_table_exp_pow_2_4 = [
        lookup_entry_exp_pow_2_4(-4),
        lookup_entry_exp_pow_2_4(-3),
        lookup_entry_exp_pow_2_4(-2),
        lookup_entry_exp_pow_2_4(-1),
        lookup_entry_exp_pow_2_4(0),
        lookup_entry_exp_pow_2_4(1),
        lookup_entry_exp_pow_2_4(2),
        lookup_entry_exp_pow_2_4(3),
    ];
    let exp_pow_2_4 = lookup_table_exp_pow_2_4[(log2 + 4) as usize];

    // Zero the exponent of x or divide by 2^log.
    let x = f32::from_bits((bits & 0x807fffff) | 0x3f800000);

    // Use lookup tables to divide by a truncated version of x and get an offset for that division.
    // x^2.4 = a^2.4 * (x/a)^2.4
    let lookup_entry_inv_truncated = |fraction: i32| {
        let truncated = 1.0 + (fraction as f64 + 0.5) / ((1 << FRAC_BITS) as f64);
        (1.0 / truncated) as f32
    };
    let lookup_table_inv_truncated = [
        lookup_entry_inv_truncated(0),
        lookup_entry_inv_truncated(1),
        lookup_entry_inv_truncated(2),
        lookup_entry_inv_truncated(3),
        lookup_entry_inv_truncated(4),
        lookup_entry_inv_truncated(5),
        lookup_entry_inv_truncated(6),
        lookup_entry_inv_truncated(7),
    ];
    let lookup_entry_truncated_pow_2_4 =
        |fraction: i32| (lookup_entry_inv_truncated(fraction) as f64).powf(-2.4) as f32;
    let lookup_table_truncated_pow_2_4 = [
        lookup_entry_truncated_pow_2_4(0),
        lookup_entry_truncated_pow_2_4(1),
        lookup_entry_truncated_pow_2_4(2),
        lookup_entry_truncated_pow_2_4(3),
        lookup_entry_truncated_pow_2_4(4),
        lookup_entry_truncated_pow_2_4(5),
        lookup_entry_truncated_pow_2_4(6),
        lookup_entry_truncated_pow_2_4(7),
    ];

    // Expose only FRAC_BITS of the fraction.
    let fraction = (bits >> (23 - FRAC_BITS) & ((1 << FRAC_BITS) - 1)) as usize;
    let truncated_pow_2_4 = lookup_table_truncated_pow_2_4[fraction];
    let x = x * lookup_table_inv_truncated[fraction];

    // Binomial series
    // Greater than 12 bits of precision.
    //let est = 7. / 25. - 24. / 25. * x + 42. / 25. * x.powi(2);
    // Plenty of precision.
    let est = 7. / 125. - 36. / 125. * x + 126. / 125. * x.powi(2) + 28. / 125. * x.powi(3);

    est * truncated_pow_2_4 * exp_pow_2_4
}
