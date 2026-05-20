use std::time::Instant;
use std::mem::swap;
use image::{DynamicImage, GenericImageView, GrayImage, ImageBuffer, ImageReader, Luma};
use rayon::prelude::*;
use tfhe::{generate_keys, set_server_key, ClientKey, ConfigBuilder, FheUint8, PublicKey};
use tfhe::prelude::{FheDecrypt, FheEncrypt, FheOrd, FheTryEncrypt, IfThenElse};

struct EncryptedImage {
    pixels: Vec<FheUint8>,
    width: usize,
    height: usize,
}

impl EncryptedImage {
    fn swap_width_height(&mut self) {
        swap(&mut self.width, &mut self.height)
    }
    fn is_square(&self) -> bool {
        self.width == self.height
    }
    fn is_large(&self) -> bool {
        image_is_large(self.width, self.height)
    }
}

fn image_is_large(width: usize, height: usize) -> bool{
    width > 512 || height > 512
}

pub(crate) fn convert_image_to_grayscale() -> Result<(), Box<dyn std::error::Error>>  {
    let img = ImageReader::open("resources/images/original.png")?.decode()?;
    let luma_img = img.to_luma8();
    // let grayscale_img = img.grayscale();
    luma_img.save("resources/images/grayscale.png")?;

    Ok(())
}

pub(crate) fn image_processing() -> Result<(), Box<dyn std::error::Error>> {
    let clean_img = ImageReader::open("resources/images/original.png")?.decode()?;

    // Client-side
    let config = ConfigBuilder::default().build();
    let (client_key, server_key) = generate_keys(config);
    let public_key = PublicKey::new(&client_key);
    rayon::broadcast(|_| set_server_key(server_key.clone()));
    set_server_key(server_key);
    print!("Keys Generated");

    let mut image = preprocess_image(clean_img, &client_key);

    // Sever-side
    let inverting = |pixel_value: &FheUint8| !pixel_value;
    let white_threshold = |pixel_value: &FheUint8| {
        pixel_value.gt(128).if_then_else(
            &FheUint8::encrypt(255u8, &public_key),
            &pixel_value
        )
    };
    // 1,6min for 256 pixels -> 0,375s per Pixel
    let black_threshold = |pixel_value: &FheUint8| {
        pixel_value.lt(128).if_then_else(
            &FheUint8::encrypt(0u8, &public_key),
            &pixel_value
        )
    };
    // edge detection (Sobel or Canny)
    // contrast (stretch/ compress values)
    // Gaussian blur
    // Posterization (only fixed number of values)
    // Solarization (Invert only above a certain intensity)
    let plus_one = |pixel_value: &FheUint8| pixel_value + 1;
    let minus_one = |pixel_value: &FheUint8| pixel_value - 1;
    let times_two = |pixel_value: &FheUint8| pixel_value * 2;
    let div_two = |pixel_value: &FheUint8| pixel_value / 2;

    apply_pixelwise_transformation(&mut image, black_threshold);

    // Client-side
    postprocess_image(
        &image.pixels,
        &client_key,
        image.width as u32,
        image.height as u32,
        "black_cutoff",
    );

    Ok(())
}

fn preprocess_image(
    clean_img: DynamicImage,
    client_key: &ClientKey
) -> EncryptedImage {
    /// returns encrypted image data and image dimensions width, height

    let width = clean_img.width() as usize;
    let height = clean_img.height() as usize;

    // maybe use into_luma8 to convert to grayscale instead?
    let raw_image_data = clean_img.to_luma8().into_raw();

    let now = Instant::now();
    // large images (e.g. above 512 x 512 fill RAM up)
    if image_is_large(width, height) {

    } else {

    }

    let pixels: Vec<FheUint8> = raw_image_data
        .par_iter()
        .map(
            |&value| FheUint8::encrypt(value, client_key),
        ).collect();
    let elapsed = now.elapsed();
    println!("Encrypt Finished in: {:?}", elapsed);
    EncryptedImage{
        pixels,
        width,
        height,
    }
}

fn apply_pixelwise_transformation<F>(
    image: &mut EncryptedImage,
    function: F
)
where
    F: Fn(&FheUint8) -> FheUint8 + Sync + Send,
{
    let now = Instant::now();
    let processed_img: Vec<FheUint8> = image.pixels
        .par_iter()
        .map(function)
        .collect();
    image.pixels = processed_img;

    let elapsed = now.elapsed();
    println!("Manipulation Finished in: {:?}", elapsed);
}

fn rotate_90(
    image: &mut EncryptedImage,
) {
    let now = Instant::now();

    if image.is_square() {
        transpose_square_matrix(image);
        flip_horizontal(image);
    } else {
        non_square_90_degrees(image)
    }
    let elapsed = now.elapsed();

    println!("90° Rotation Finished in: {:?}", elapsed);

}

fn rotate_180(
    image: &mut EncryptedImage,
) {
    let now = Instant::now();

    flip_horizontal(image);
    flip_vertical(image);

    let elapsed = now.elapsed();
    println!("180° Rotation Finished in: {:?}", elapsed);
}

fn rotate_270(
    image: &mut EncryptedImage,
) {
    let now = Instant::now();

    if image.is_square() {
        transpose_square_matrix(image);
        flip_vertical(image);
    } else {
        non_square_270_degrees(image)
    }
    let elapsed = now.elapsed();
    println!("270° Rotation Finished in: {:?}", elapsed);
}

fn flip_horizontal(
    image: &mut EncryptedImage,
) {
    let now = Instant::now();

    for row in image.pixels.chunks_mut(image.width) {
        row.reverse();
    }

    let elapsed = now.elapsed();
    println!("Horizontal-Flip Finished in: {:?}", elapsed);
}

fn flip_vertical(
    image: &mut EncryptedImage,
) {
    let now = Instant::now();

    // swaps top and bottom row and removes them, until all rows processed
    let mut data = &mut image.pixels[..];
    // while two rows remaining
    while data.len() >= image.width * 2 {
        let (top_row, rest) = data.split_at_mut(image.width);
        let (middle, bottom_row) = rest.split_at_mut(rest.len() - image.width);

        top_row.swap_with_slice(bottom_row);
        data = middle;
    }
    let elapsed = now.elapsed();
    println!("Vertical-Flip Finished in: {:?}", elapsed);
}

fn postprocess_image(
    processed_enc_img_data: &Vec<FheUint8>,
    client_key: &ClientKey,
    width: u32,
    height: u32,
    img_name: &str
) -> ImageBuffer<Luma<u8>, Vec<u8>> {
    let now = Instant::now();

    let plain_processed_img: Vec<u8> = processed_enc_img_data
        .par_iter()
        .map(
            |value| value.decrypt(&client_key),
        ).collect();
    let elapsed = now.elapsed();
    println!("Decrypt Finished in: {:?}", elapsed);

    let processed_img = GrayImage::from_raw(width, height, plain_processed_img).unwrap();

    let img_location = format!("resources/images/processed/{}.png", img_name);
    processed_img.save(img_location).expect("TODO: panic message");

    println!("Storing Image finished: {:?}", now.elapsed());
    processed_img
}

fn transpose_square_matrix(
    image: &mut EncryptedImage,
) {
    let size = image.width;
    // Index entry by row * size + col
    // Only iterate "upper" triangle (with row+1..size)
    for row in 0..size {
        for column in row+1..size {
            let source_index = row * size + column;
            let destination_index = column * size + row;
            image.pixels.swap(source_index, destination_index);
        }
    }
}

/// TODO() Check if clone overhead is better
fn non_square_90_degrees(
    image: &mut EncryptedImage
) {
    let width = image.width;
    let height = image.height;
    // idea, create new array and pick every element
    // we do this to avoid cloning
    let mut new_encrypted_image_data: Vec<FheUint8> = Vec::with_capacity(width * height);

    // loop through new vec so that values are stored next to each other in memory
    for new_row in 0..width {
        for new_column in 0..height {
            // calculate index where to take value from
            let old_row = height - 1 - new_column;
            let old_column = new_row;

            // height and width reverse due to rotation
            let old_index = old_row * width + old_column;

            new_encrypted_image_data.push(image.pixels[old_index].clone())
        }
    }
    image.swap_width_height();
    image.pixels = new_encrypted_image_data;
}

fn non_square_270_degrees(
    image: &mut EncryptedImage
) {
    let width = image.width;
    let height = image.height;
    // idea, create new array and pick every element
    // we do this to avoid cloning
    let mut new_encrypted_image_data: Vec<FheUint8> = Vec::with_capacity(width * height);

    // loop through new vec so that values are stored next to each other in memory
    for new_row in 0..width {
        for new_column in 0..height {
            // calculate index where to take value from
            let old_row = new_column;
            let old_column = new_row;

            // height and width reverse due to rotation
            let old_index = old_row * width + old_column;

            new_encrypted_image_data.push(image.pixels[old_index].clone())
        }
    }
    image.swap_width_height();
    image.pixels = new_encrypted_image_data;
}
