extern crate clap;

use clap::{App, Arg, ArgMatches};
use std::fs::File;
use std::io::prelude::*;

use std::process::exit;

use delta_e::*;

pub struct CliOptions {
    pub input1: Box<dyn Read>,
    pub input2: Box<dyn Read>,
    pub summary: bool,
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
            Arg::with_name("SUMMARY")
                .help("Only output the summary line")
                .short("s")
                .long("summary"),
        )
        .get_matches();
    CliOptions {
        input1: Box::new(File::open(matches.value_of("video1").unwrap()).unwrap()) as Box<dyn Read>,
        input2: Box::new(File::open(matches.value_of("video2").unwrap()).unwrap()) as Box<dyn Read>,
        summary: matches.is_present("SUMMARY"),
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
    let (bit_depth, bytewidth) = {
        /*let colorspace1 = video1.get_colorspace();
        let colorspace2 = video2.get_colorspace();*/
        let bit_depth1 = video1.get_bit_depth();
        let bit_depth2 = video2.get_bit_depth();
        if bit_depth1 != bit_depth2 {
            eprintln!("Bit depths do not match: {} != {}", bit_depth1, bit_depth2);
            exit(1);
        }
        // TODO: get and test chroma sampling
        (bit_depth1, video1.get_bytes_per_sample())
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

    //let y_stride = width * bytewidth;
    let sample_max = (1 << bit_depth) - 1;
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
                    match bytewidth {
                        1 => {
                            let y_row1 = &y_plane1[i * width..];
                            let u_row1 = &u_plane1[(i >> 1) * (width >> 1)..];
                            let v_row1 = &v_plane1[(i >> 1) * (width >> 1)..];
                            let y_row2 = &y_plane2[i * width..];
                            let u_row2 = &u_plane2[(i >> 1) * (width >> 1)..];
                            let v_row2 = &v_plane2[(i >> 1) * (width >> 1)..];
                            for j in 0..width {
                                let yuv_to_rgb = |y: f32, u: f32, v: f32| {
                                    let y = (y - 16.) * (1. / 219.);
                                    let u = (u - 128.) * (1. / 224.);
                                    let v = (v - 128.) * (1. / 224.);

                                    let r = y + 1.28033 * v;
                                    let g = y - 0.21482 * u - 0.38059 * v;
                                    let b = y + 2.12798 * u;

                                    (r, g, b)
                                };

                                let (r1, g1, b1) = yuv_to_rgb(
                                    y_row1[j] as f32,
                                    u_row1[j >> 1] as f32,
                                    v_row1[j >> 1] as f32,
                                );
                                let (r2, g2, b2) = yuv_to_rgb(
                                    y_row2[j] as f32,
                                    u_row2[j >> 1] as f32,
                                    v_row2[j >> 1] as f32,
                                );
                                delta_e_vec[i * width + j] = DE2000::from_rgb_f32(
                                    &[r1, g1, b1],
                                    &[r2, g2, b2],
                                );
                            }
                        }
                        _ => {}
                    }
                }
                let score = 45. - 20. * (delta_e_vec.iter().map(|x| *x as f64).sum::<f64>() / ((width * height) as f64)).log10();
                println!("{:08}: {:2.4}", num_frames, score);
                total += score;
                num_frames += 1;
            }
            _ => {
                break;
            }
        }
    }
    println!("Total: {:2.4}", total / (num_frames as f64));
}
