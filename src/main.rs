// BSD 2-Clause License
//
// Copyright (c) 2019, the dump_ciede2000 contributors
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are met:
//
// * Redistributions of source code must retain the above copyright notice, this
//  list of conditions and the following disclaimer.
//
// * Redistributions in binary form must reproduce the above copyright notice,
//  this list of conditions and the following disclaimer in the documentation
//  and/or other materials provided with the distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
// AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
// IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
// FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
// DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
// SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
// CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
// OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
// OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

extern crate clap;

#[macro_use]
extern crate itertools;

use clap::{App, Arg};
use std::fs::File;
use std::io::prelude::*;

use std::process::exit;

mod rgbtolab;
use rgbtolab::*;

mod delta_e;
use delta_e::*;

struct CliOptions {
    pub input1: Box<dyn Read>,
    pub input2: Box<dyn Read>,
    pub summary: bool,
    pub limit: Option<usize>,
    pub simd: bool,
}

fn parse_cli() -> CliOptions {
    let matches = App::new("fast_ciede2000")
        .about("Video quality metric based off color difference instead of just luma or chroma")
        .arg(
            Arg::with_name("video1")
                .help("Uncompressed YUV4MPEG2 video input")
                .required(true),
        )
        .arg(
            Arg::with_name("video2")
                .help("Uncompressed YUV4MPEG2 video input")
                .required(true),
        )
        .arg(
            Arg::with_name("LIMIT")
                .help("Maximum number of frames to process")
                .short('l')
                .long("limit")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("SUMMARY")
                .help("Only output the summary line")
                .short('s')
                .long("summary"),
        )
        .arg(
            Arg::with_name("SIMD")
                .help("Set simd feature level")
                .long("simd")
                .takes_value(true)
                .possible_values(&["off", "native"])
                .default_value("native"),
        )
        .arg(
            Arg::with_name("THREADS")
                .help("Set threadpool size (unimplemented)")
                .long("threads")
                .takes_value(true),
        )
        .get_matches();
    CliOptions {
        input1: Box::new(File::open(matches.value_of("video1").unwrap()).unwrap()) as Box<dyn Read>,
        input2: Box::new(File::open(matches.value_of("video2").unwrap()).unwrap()) as Box<dyn Read>,
        summary: matches.is_present("SUMMARY"),
        limit: matches
            .value_of("LIMIT")
            .map(|v| v.parse().expect("Limit must be a positive number")),
        simd: match matches.value_of("SIMD").unwrap() {
            "off" => false,
            "native" => true,
            &_ => unreachable!(),
        },
    }
}

// Taken from rav1e
#[derive(Copy, Clone, Debug, PartialEq)]
enum ChromaSampling {
    Cs420,
    Cs422,
    Cs444,
    Cs400,
}

// Taken from rav1e
fn map_y4m_color_space(color_space: y4m::Colorspace) -> ChromaSampling {
    use y4m::Colorspace::*;
    use ChromaSampling::*;
    match color_space {
        Cmono => Cs400,
        C420jpeg | C420paldv => Cs420,
        C420mpeg2 => Cs420,
        C420 | C420p10 | C420p12 => Cs420,
        C422 | C422p10 | C422p12 => Cs422,
        C444 | C444p10 | C444p12 => Cs444,
    }
}

fn main() {
    let mut cli = parse_cli();
    let mut video1 = y4m::decode(&mut cli.input1).unwrap();
    let mut video2 = y4m::decode(&mut cli.input2).unwrap();
    let (width, height) = {
        let dimension1 = (video1.get_width(), video1.get_height());
        let dimension2 = (video2.get_width(), video2.get_height());

        if dimension1 != dimension2 {
            eprintln!(
                "Video dimensions do not match: {}x{} != {}x{}",
                dimension1.0, dimension1.1, dimension2.0, dimension2.1
            );
            exit(1);
        }
        dimension1
    };
    let (bit_depth, bytewidth, xdec, ydec) = {
        let colorspace1 = video1.get_colorspace();
        let colorspace2 = video2.get_colorspace();
        let bit_depth1 = colorspace1.get_bit_depth();
        let bit_depth2 = colorspace2.get_bit_depth();
        if bit_depth1 != bit_depth2 {
            eprintln!("Bit depths do not match: {} != {}", bit_depth1, bit_depth2);
            exit(1);
        }
        let sampling1 = map_y4m_color_space(colorspace1);
        let sampling2 = map_y4m_color_space(colorspace2);
        if sampling1 != sampling2 {
            eprintln!("Sub sampling does not match. Mismatched subsampling is not supported.");
            exit(1);
        }
        if sampling1 == ChromaSampling::Cs400 {
            eprintln!("Grayscale is unsupported.")
        }
        let (xdec, ydec) = {
            use self::ChromaSampling::*;
            match sampling1 {
                Cs420 => (1, 1),
                Cs422 => (1, 0),
                Cs444 => (0, 0),
                Cs400 => (1, 1),
            }
        };
        (bit_depth1, video1.get_bytes_per_sample(), xdec, ydec)
    };
    {
        let framerate1 = video1.get_framerate();
        let framerate2 = video2.get_framerate();
        if framerate1.num * framerate2.den != framerate2.num * framerate1.den {
            eprintln!(
                "Warning - Framerates do not match: {} != {}",
                framerate1, framerate2
            );
        }
    }

    // luma stride
    let y_stride = width * bytewidth;
    // chroma stride
    let c_stride = (width >> xdec) * bytewidth;
    let delta_e_row_fn = get_delta_e_row_fn(bit_depth, xdec, cli.simd);
    let mut num_frames: usize = 0;
    let mut total: f64 = 0f64;
    loop {
        match (video1.read_frame(), video2.read_frame()) {
            (Ok(pic1), Ok(pic2)) => {
                let mut delta_e_vec: Vec<f32> = vec![0.0; width * height];
                let y_plane1 = pic1.get_y_plane();
                let u_plane1 = pic1.get_u_plane();
                let v_plane1 = pic1.get_v_plane();
                let y_plane2 = pic2.get_y_plane();
                let u_plane2 = pic2.get_u_plane();
                let v_plane2 = pic2.get_v_plane();
                for i in 0..height {
                    unsafe {
                        delta_e_row_fn(
                            FrameRow {
                                y: &y_plane1[i * y_stride..][..y_stride],
                                u: &u_plane1[(i >> ydec) * c_stride..][..c_stride],
                                v: &v_plane1[(i >> ydec) * c_stride..][..c_stride],
                            },
                            FrameRow {
                                y: &y_plane2[i * y_stride..][..y_stride],
                                u: &u_plane2[(i >> ydec) * c_stride..][..c_stride],
                                v: &v_plane2[(i >> ydec) * c_stride..][..c_stride],
                            },
                            &mut delta_e_vec[i * width..][..width],
                        );
                    }
                }
                let score = 45.
                    - 20.
                        * (delta_e_vec.iter().map(|x| *x as f64).sum::<f64>()
                            / ((width * height) as f64))
                            .log10();
                total += score;
                if !cli.summary {
                    println!("{:08}: {:2.4}", num_frames, score);
                }
                num_frames += 1;
                if let Some(limit) = cli.limit {
                    if num_frames >= limit {
                        break;
                    }
                }
            }
            _ => {
                break;
            }
        }
    }
    println!("Total: {:2.4}", total / (num_frames as f64));
}

// Arguments for delta e
// "Color Image Quality Assessment Based on CIEDE2000"
// Yang Yang, Jun Ming and Nenghai Yu, 2012
// http://dx.doi.org/10.1155/2012/273723
const K_SUB: KSubArgs = KSubArgs {
    l: 0.65,
    c: 1.0,
    h: 4.0,
};

pub struct FrameRow<'a> {
    y: &'a [u8],
    u: &'a [u8],
    v: &'a [u8],
}

type DeltaERowFn = unsafe fn(FrameRow, FrameRow, &mut [f32]);

fn get_delta_e_row_fn(bit_depth: usize, xdec: usize, simd: bool) -> DeltaERowFn {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if is_x86_feature_detected!("avx2") && xdec == 1 && simd {
            return match bit_depth {
                8 => BD8::delta_e_row_avx2,
                10 => BD10::delta_e_row_avx2,
                12 => BD12::delta_e_row_avx2,
                _ => unreachable!(),
            };
        }
    }
    match (bit_depth, xdec) {
        (8, 1) => BD8::delta_e_row_scalar,
        (10, 1) => BD10::delta_e_row_scalar,
        (12, 1) => BD12::delta_e_row_scalar,
        (8, 0) => BD8_444::delta_e_row_scalar,
        (10, 0) => BD10_444::delta_e_row_scalar,
        (12, 0) => BD12_444::delta_e_row_scalar,
        _ => unreachable!(),
    }
}

pub trait Colorspace {
    const BIT_DEPTH: u32;
    const X_DECIMATION: u32;
}

struct BD8;
struct BD10;
struct BD12;

struct BD8_444;
struct BD10_444;
struct BD12_444;

impl Colorspace for BD8 {
    const BIT_DEPTH: u32 = 8;
    const X_DECIMATION: u32 = 1;
}
impl Colorspace for BD10 {
    const BIT_DEPTH: u32 = 10;
    const X_DECIMATION: u32 = 1;
}
impl Colorspace for BD12 {
    const BIT_DEPTH: u32 = 12;
    const X_DECIMATION: u32 = 1;
}
impl Colorspace for BD8_444 {
    const BIT_DEPTH: u32 = 8;
    const X_DECIMATION: u32 = 0;
}
impl Colorspace for BD10_444 {
    const BIT_DEPTH: u32 = 10;
    const X_DECIMATION: u32 = 0;
}
impl Colorspace for BD12_444 {
    const BIT_DEPTH: u32 = 12;
    const X_DECIMATION: u32 = 0;
}

fn twice<T>(
    i: T,
) -> itertools::Interleave<<T as IntoIterator>::IntoIter, <T as IntoIterator>::IntoIter>
where
    T: IntoIterator + Clone,
{
    itertools::interleave(i.clone(), i)
}

pub trait DeltaEScalar: Colorspace {
    fn delta_e_scalar(yuv1: (u16, u16, u16), yuv2: (u16, u16, u16)) -> f32 {
        let scale = (1 << (Self::BIT_DEPTH - 8)) as f32;
        let yuv_to_rgb = |yuv: (u16, u16, u16)| {
            // Assumes BT.709
            let y = (yuv.0 as f32 - 16. * scale) * (1. / (219. * scale));
            let u = (yuv.1 as f32 - 128. * scale) * (1. / (224. * scale));
            let v = (yuv.2 as f32 - 128. * scale) * (1. / (224. * scale));

            // [-0.804677, 1.81723]
            let r = y + 1.28033 * v;
            // [−0.316650, 1.09589]
            let g = y - 0.21482 * u - 0.38059 * v;
            // [-1.28905, 2.29781]
            let b = y + 2.12798 * u;

            (r, g, b)
        };

        let (r1, g1, b1) = yuv_to_rgb(yuv1);
        let (r2, g2, b2) = yuv_to_rgb(yuv2);
        DE2000::new(rgb_to_lab(&[r1, g1, b1]), rgb_to_lab(&[r2, g2, b2]), K_SUB)
    }

    unsafe fn delta_e_row_scalar(row1: FrameRow, row2: FrameRow, res_row: &mut [f32]) {
        // Only one version should be compiled for each trait
        if Self::BIT_DEPTH == 8 {
            if Self::X_DECIMATION == 1 {
                for (y1, u1, v1, y2, u2, v2, res) in izip!(
                    row1.y,
                    twice(row1.u),
                    twice(row1.v),
                    row2.y,
                    twice(row2.u),
                    twice(row2.v),
                    res_row
                ) {
                    *res = Self::delta_e_scalar(
                        (*y1 as u16, *u1 as u16, *v1 as u16),
                        (*y2 as u16, *u2 as u16, *v2 as u16),
                    );
                }
            } else {
                for (y1, u1, v1, y2, u2, v2, res) in
                    izip!(row1.y, row1.u, row1.v, row2.y, row2.u, row2.v, res_row)
                {
                    *res = Self::delta_e_scalar(
                        (*y1 as u16, *u1 as u16, *v1 as u16),
                        (*y2 as u16, *u2 as u16, *v2 as u16),
                    );
                }
            }
        } else {
            if Self::X_DECIMATION == 1 {
                for (y1, u1, v1, y2, u2, v2, res) in izip!(
                    row1.y.chunks(2),
                    twice(row1.u.chunks(2)),
                    twice(row1.v.chunks(2)),
                    row2.y.chunks(2),
                    twice(row2.u.chunks(2)),
                    twice(row2.v.chunks(2)),
                    res_row
                ) {
                    let to_u16 =
                        |input: &[u8]| -> u16 { ((input[1] as u16) << 8) | (input[0] as u16) };
                    *res = Self::delta_e_scalar(
                        (to_u16(&*y1), to_u16(&*u1), to_u16(&*v1)),
                        (to_u16(&*y2), to_u16(&*u2), to_u16(&*v2)),
                    );
                }
            } else {
                for (y1, u1, v1, y2, u2, v2, res) in izip!(
                    row1.y.chunks(2),
                    row1.u.chunks(2),
                    row1.v.chunks(2),
                    row2.y.chunks(2),
                    row2.u.chunks(2),
                    row2.v.chunks(2),
                    res_row
                ) {
                    let to_u16 =
                        |input: &[u8]| -> u16 { ((input[1] as u16) << 8) | (input[0] as u16) };
                    *res = Self::delta_e_scalar(
                        (to_u16(&*y1), to_u16(&*u1), to_u16(&*v1)),
                        (to_u16(&*y2), to_u16(&*u2), to_u16(&*v2)),
                    );
                }
            }
        }
    }
}

impl DeltaEScalar for BD8 {}
impl DeltaEScalar for BD10 {}
impl DeltaEScalar for BD12 {}
impl DeltaEScalar for BD8_444 {}
impl DeltaEScalar for BD10_444 {}
impl DeltaEScalar for BD12_444 {}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use self::avx2::*;

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
mod avx2 {
    use super::*;

    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    pub trait DeltaEAVX2: Colorspace + DeltaEScalar {
        #[target_feature(enable = "avx2")]
        unsafe fn yuv_to_rgb(yuv: (__m256, __m256, __m256)) -> (__m256, __m256, __m256) {
            let scale: f32 = (1 << (Self::BIT_DEPTH - 8)) as f32;
            #[target_feature(enable = "avx2")]
            unsafe fn set1(val: f32) -> __m256 {
                _mm256_set1_ps(val)
            };
            let y = _mm256_mul_ps(
                _mm256_sub_ps(yuv.0, set1(16. * scale)),
                set1(1. / (219. * scale)),
            );
            let u = _mm256_mul_ps(
                _mm256_sub_ps(yuv.1, set1(128. * scale)),
                set1(1. / (224. * scale)),
            );
            let v = _mm256_mul_ps(
                _mm256_sub_ps(yuv.2, set1(128. * scale)),
                set1(1. / (224. * scale)),
            );

            let r = _mm256_add_ps(y, _mm256_mul_ps(v, set1(1.28033)));
            let g = _mm256_add_ps(
                _mm256_add_ps(y, _mm256_mul_ps(u, set1(-0.21482))),
                _mm256_mul_ps(v, set1(-0.38059)),
            );
            let b = _mm256_add_ps(y, _mm256_mul_ps(u, set1(2.12798)));

            (r, g, b)
        }

        #[target_feature(enable = "avx2")]
        unsafe fn delta_e_avx2(
            yuv1: (__m256, __m256, __m256),
            yuv2: (__m256, __m256, __m256),
            res_chunk: &mut [f32],
        ) {
            let (r1, g1, b1) = Self::yuv_to_rgb(yuv1);
            let (r2, g2, b2) = Self::yuv_to_rgb(yuv2);

            let lab1 = rgb_to_lab_avx2(&[r1, g1, b1]);
            let lab2 = rgb_to_lab_avx2(&[r2, g2, b2]);
            for i in 0..8 {
                res_chunk[i] = DE2000::new(lab1[i], lab2[i], K_SUB);
            }
        }

        #[target_feature(enable = "avx2")]
        unsafe fn delta_e_row_avx2(row1: FrameRow, row2: FrameRow, res_row: &mut [f32]) {
            // Only one version should be compiled for each trait
            if Self::BIT_DEPTH == 8 {
                for (chunk1_y, chunk1_u, chunk1_v, chunk2_y, chunk2_u, chunk2_v, res_chunk) in izip!(
                    row1.y.chunks(8),
                    row1.u.chunks(4),
                    row1.v.chunks(4),
                    row2.y.chunks(8),
                    row2.u.chunks(4),
                    row2.v.chunks(4),
                    res_row.chunks_mut(8)
                ) {
                    if chunk1_y.len() == 8 {
                        #[target_feature(enable = "avx2")]
                        unsafe fn load_luma(chunk: &[u8]) -> __m256 {
                            let tmp = _mm_loadl_epi64(chunk.as_ptr() as *const _);
                            _mm256_cvtepi32_ps(_mm256_cvtepu8_epi32(tmp))
                        };

                        #[target_feature(enable = "avx2")]
                        unsafe fn load_chroma(chunk: &[u8]) -> __m256 {
                            let tmp = _mm_cvtsi32_si128(*(chunk.as_ptr() as *const i32));
                            _mm256_cvtepi32_ps(_mm256_cvtepu8_epi32(_mm_unpacklo_epi8(tmp, tmp)))
                        };

                        Self::delta_e_avx2(
                            (
                                load_luma(chunk1_y),
                                load_chroma(chunk1_u),
                                load_chroma(chunk1_v),
                            ),
                            (
                                load_luma(chunk2_y),
                                load_chroma(chunk2_u),
                                load_chroma(chunk2_v),
                            ),
                            res_chunk,
                        );
                    } else {
                        Self::delta_e_row_scalar(
                            FrameRow {
                                y: chunk1_y,
                                u: chunk1_u,
                                v: chunk1_v,
                            },
                            FrameRow {
                                y: chunk2_y,
                                u: chunk2_u,
                                v: chunk2_v,
                            },
                            res_chunk,
                        );
                    }
                }
            } else {
                for (chunk1_y, chunk1_u, chunk1_v, chunk2_y, chunk2_u, chunk2_v, res_chunk) in izip!(
                    row1.y.chunks(16),
                    row1.u.chunks(8),
                    row1.v.chunks(8),
                    row2.y.chunks(16),
                    row2.u.chunks(8),
                    row2.v.chunks(8),
                    res_row.chunks_mut(8)
                ) {
                    if chunk1_y.len() == 16 {
                        #[target_feature(enable = "avx2")]
                        unsafe fn load_luma(chunk: &[u8]) -> __m256 {
                            let tmp = _mm_loadu_si128(chunk.as_ptr() as *const _);
                            _mm256_cvtepi32_ps(_mm256_cvtepu16_epi32(tmp))
                        };

                        #[target_feature(enable = "avx2")]
                        unsafe fn load_chroma(chunk: &[u8]) -> __m256 {
                            let tmp = _mm_loadl_epi64(chunk.as_ptr() as *const _);
                            _mm256_cvtepi32_ps(_mm256_cvtepu16_epi32(_mm_unpacklo_epi16(tmp, tmp)))
                        };

                        Self::delta_e_avx2(
                            (
                                load_luma(chunk1_y),
                                load_chroma(chunk1_u),
                                load_chroma(chunk1_v),
                            ),
                            (
                                load_luma(chunk2_y),
                                load_chroma(chunk2_u),
                                load_chroma(chunk2_v),
                            ),
                            res_chunk,
                        );
                    } else {
                        Self::delta_e_row_scalar(
                            FrameRow {
                                y: chunk1_y,
                                u: chunk1_u,
                                v: chunk1_v,
                            },
                            FrameRow {
                                y: chunk2_y,
                                u: chunk2_u,
                                v: chunk2_v,
                            },
                            res_chunk,
                        );
                    }
                }
            }
        }
    }

    impl DeltaEAVX2 for BD8 {}
    impl DeltaEAVX2 for BD10 {}
    impl DeltaEAVX2 for BD12 {}
}
