use std::mem::swap;
use std::time::Instant;
use axum_test::expect_json::__private::serde_trampoline::{Deserialize, Serialize};
use rayon::prelude::*;
use tfhe::FheUint8;
use tfhe::prelude::{FheOrd, FheTrivialEncrypt, IfThenElse};
use tfhe::shortint::backward_compatibility::server_key;

#[derive(Serialize, Deserialize)]
pub struct EncryptedImage {
    pub(crate) pixels: Vec<FheUint8>,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl EncryptedImage {
    // PER_PIXEL
    fn base_func<F>(
        &mut self,
        function: F
    )
    where
        F: Fn(&FheUint8) -> FheUint8 + Sync + Send,
    {
        let now = Instant::now();
        let processed_img: Vec<FheUint8> = self.pixels
            .par_iter()
            .map(function)
            .collect();
        self.pixels = processed_img;

        let elapsed = now.elapsed();
        println!("Manipulation Finished in: {:?}", elapsed);
    }

    // Per Pixel extended by access to neighbors
    fn extended_base_func<F>(
        &mut self,
        function: F
    ) 
    where 
        F: Fn(u32, u32, &EncryptedImage) -> FheUint8 + Sync + Send,
    {
        let now = Instant::now();

        let width = self.width;
        let height = self.height;

        let processed: Vec<FheUint8> = (0..width * height)
            .into_par_iter()
            .map(|index| {
                let row = index / width;
                let col = index % width;
                function(row, col, self)
            })
            .collect();
        self.pixels = processed;

        println!(
            "Neighborhood manipulation finished in {:?}",
            now.elapsed()
        );
    }

    fn get_pixel(&self, row: u32, col: u32) -> &FheUint8 {
        let index = (row * self.width + col) as usize;
        &self.pixels[index]
    }

    pub fn invert(&mut self) {
        let inverting = |pixel_value: &FheUint8| !pixel_value;

        self.base_func(inverting);
    }

    pub fn white_threshold(&mut self) {
        let white_threshold = |pixel_value: &FheUint8| {
            pixel_value.gt(128).if_then_else(
                &FheUint8::encrypt_trivial(255u8),
                &pixel_value
            )
        };

        self.base_func(white_threshold);
    }

    pub fn black_threshold(&mut self) {
        let black_threshold = |pixel_value: &FheUint8| {
            pixel_value.lt(128).if_then_else(
                &FheUint8::encrypt_trivial(0u8),
                &pixel_value
            )
        };

        self.base_func(black_threshold);
    }
    
    // EFFECTS
    fn map_pixels<F>(&self, function: F) -> EncryptedImage
    where
        F: Fn(&FheUint8) -> FheUint8 + Sync + Send  
    {
        let pixels = self.pixels.par_iter().map(function).collect();
        EncryptedImage { pixels, width: self.width, height: self.height }
    }
    
    fn add_image(&mut self, other: &EncryptedImage) {
        let max = FheUint8::encrypt_trivial(255u8);
        self.extended_base_func( |row, col, image| {
            let sum = image.get_pixel(row, col) + other.get_pixel(row, col);
            sum.gt(255u8).if_then_else(&max, &sum)
        });
    }

    fn create_lightmap(&self) -> EncryptedImage {
        let theshhold = 220u8;
        let zero = FheUint8::encrypt_trivial(0u8);

        self.map_pixels(|pixel| {
            pixel.lt(theshhold).if_then_else(&zero, pixel)
        })
    }
    
    pub fn blooming(&mut self) {
        let mut lightmap = self.create_lightmap();
        lightmap.box_blur_splitted();
        self.add_image(&lightmap);
    }

    pub fn box_blur_simple(&mut self) {
        let width = self.width;
        self.extended_base_func(|row, col, image| {
            if row == 0 || col == 0 ||row == image.height - 1 || col == image.width - 1 {
                return image.get_pixel(row, col).clone();
            }
            let index = row * width + col;
            let pixels = &image.pixels;
            let center =  &pixels[index as usize];

            let n = &pixels[(index - width) as usize];
            let s = &pixels[(index + width) as usize];
            let e = &pixels[(index + 1) as usize];
            let w = &pixels[(index - 1) as usize];

            let cent = center >> 2u8;
            let card1 = (n >> 3u8) + (e >> 3u8);
            let card2 = (s >> 3u8) + (w >> 3u8);

            cent + (card1 + card2)
    });}

    pub fn box_blur_weighted(&mut self) {
        let width = self.width;
        self.extended_base_func(|row, col, image| {
            let index = row * width + col;
            if row == 0 || col == 0 ||row == image.height - 1 || col == image.width - 1 {
                return image.get_pixel(row, col).clone();
            }
            
            let pixels = &image.pixels;
            let center =  &pixels[index as usize];

            let n = &pixels[(index - width) as usize];
            let s = &pixels[(index + width) as usize];
            let e = &pixels[(index + 1) as usize];
            let w = &pixels[(index - 1) as usize];
            
            let ne = &pixels[(index - width + 1) as usize];
            let nw = &pixels[(index - width - 1) as usize];
            let se = &pixels[(index + width + 1) as usize];
            let sw = &pixels[(index + width - 1) as usize];

            let cent = center >> 2u8;
            let card1 = (n >> 3u8) + (e >> 3u8);
            let card2 = (s >> 3u8) + (w >> 3u8);
            let diag1 = (ne >> 4u8) + (nw >> 4u8);
            let diag2 = (se >> 4u8) + (sw >> 4u8);

            cent + (card1 + card2) + (diag1 + diag2)
    });}

    pub fn box_blur_splitted(&mut self) {
        self.box_blur_horizontal();
        self.box_blur_vertical();
    }

    pub fn box_blur_horizontal(&mut self) {
        let width = self.width;
        self.extended_base_func(|row, col, image| {
            if col == 0 || col == image.width - 1 {
                return image.get_pixel(row, col).clone();
            }
            let index = row * width + col;
            let pixels = &image.pixels;
            let left = &pixels[(index - 1) as usize];
            let center =  &pixels[index as usize];
            let right = &pixels[(index + 1) as usize];
            (left >> 2u8) + (center >> 1u8) + (right >> 2u8)
        });
    }

    pub fn box_blur_vertical(&mut self) {
        let width = self.width;
        self.extended_base_func(|row, col, image| {
            if row == 0 || row == image.height - 1 {
                return image.get_pixel(row, col).clone();
            }
            let index = row * width + col;
            let pixels = &image.pixels;
            let top = &pixels[(index - width) as usize];
            let center =  &pixels[index as usize];
            let bottom = &pixels[(index + width) as usize];
            (top >> 2u8) + (center >> 1u8) + (bottom >> 2u8)
        });
    }

    pub fn blooming_per_pixel(&mut self) {
        let strong = FheUint8::encrypt_trivial(20u8);
        let weak = FheUint8::encrypt_trivial(10u8);
        let zero = FheUint8::encrypt_trivial(0u8);
        let threshhold = 220u8;

        self.extended_base_func(|row, col, image| {
            if row == 0 || col == 0 ||row == image.height - 1 || col == image.width - 1 {
                return image.get_pixel(row, col).clone();
            }

            let center = image.get_pixel(row, col);
            
            let n  = image.get_pixel(row - 1, col);
            let s  = image.get_pixel(row + 1, col);
            let e  = image.get_pixel(row, col + 1);
            let w  = image.get_pixel(row, col - 1);
            
            let ne = image.get_pixel(row - 1, col + 1);
            let nw = image.get_pixel(row - 1, col - 1);
            let se = image.get_pixel(row + 1, col + 1);
            let sw = image.get_pixel(row + 1, col - 1);
            
            let boost = 
                n.gt(threshhold).if_then_else(&strong, &zero)
                + s.gt(threshhold).if_then_else(&strong, &zero)
                + e.gt(threshhold).if_then_else(&strong, &zero)
                + w.gt(threshhold).if_then_else(&strong, &zero)
                + ne.gt(threshhold).if_then_else(&weak, &zero)
                + nw.gt(threshhold).if_then_else(&weak, &zero)
                + se.gt(threshhold).if_then_else(&weak, &zero)
                + sw.gt(threshhold).if_then_else(&weak, &zero);
            
            center + boost
    });}

    // ROTATIONS:
    pub fn rotate_90(&mut self) {
        let now = Instant::now();

        if self.is_square() {
            self.transpose_square_matrix();
            self.flip_horizontal();
        } else {
            self.non_square_90_degrees()
        }
        let elapsed = now.elapsed();

        println!("90° Rotation Finished in: {:?}", elapsed);

    }

    pub fn rotate_180(&mut self) {
        let now = Instant::now();

        self.flip_horizontal();
        self.flip_vertical();

        let elapsed = now.elapsed();
        println!("180° Rotation Finished in: {:?}", elapsed);
    }

    pub fn rotate_270(&mut self) {
        let now = Instant::now();

        if self.is_square() {
            self.transpose_square_matrix();
            self.flip_vertical();
        } else {
            self.non_square_270_degrees()
        }
        let elapsed = now.elapsed();
        println!("270° Rotation Finished in: {:?}", elapsed);
    }

    fn transpose_square_matrix(&mut self) {
        let size = self.width;
        // Index entry by row * size + col
        // Only iterate "upper" triangle (with row+1..size)
        for row in 0..size {
            for column in row+1..size {
                let source_index = row * size + column;
                let destination_index = column * size + row;
                self.pixels.swap(source_index as usize, destination_index as usize);
            }
        }
    }

    /// TODO() Check if clone overhead is better
    fn non_square_90_degrees(&mut self) {
        let width = self.width;
        let height = self.height;
        // idea, create new array and pick every element
        // we do this to avoid cloning
        let mut new_encrypted_image_data: Vec<FheUint8> = Vec::with_capacity((width * height) as usize);

        // loop through new vec so that values are stored next to each other in memory
        for new_row in 0..width {
            for new_column in 0..height {
                // calculate index where to take value from
                let old_row = height - 1 - new_column;
                let old_column = new_row;

                // height and width reverse due to rotation
                let old_index = (old_row * width + old_column) as usize;

                new_encrypted_image_data.push(self.pixels[old_index].clone())
            }
        }
        self.swap_width_height();
        self.pixels = new_encrypted_image_data;
    }

    fn non_square_270_degrees(&mut self) {
        let width = self.width;
        let height = self.height;
        // idea, create new array and pick every element
        // we do this to avoid cloning
        let mut new_encrypted_image_data: Vec<FheUint8> = Vec::with_capacity((width * height) as usize);

        // loop through new vec so that values are stored next to each other in memory
        for new_row in 0..width {
            for new_column in 0..height {
                // calculate index where to take value from
                let old_row = new_column;
                let old_column = new_row;

                // height and width reverse due to rotation
                let old_index = (old_row * width + old_column) as usize;

                new_encrypted_image_data.push(self.pixels[old_index].clone())
            }
        }
        self.swap_width_height();
        self.pixels = new_encrypted_image_data;
    }

    pub fn flip_horizontal(&mut self) {
        let now = Instant::now();

        for row in self.pixels.chunks_mut(self.width as usize) {
            row.reverse();
        }

        let elapsed = now.elapsed();
        println!("Horizontal-Flip Finished in: {:?}", elapsed);
    }

    pub fn flip_vertical(&mut self) {
        let now = Instant::now();

        // swaps top and bottom row and removes them, until all rows processed
        let mut data = &mut self.pixels[..];
        // while two rows remaining
        while data.len() >= (self.width * 2) as usize {
            let (top_row, rest) = data.split_at_mut(self.width as usize);
            let (middle, bottom_row) = rest.split_at_mut(rest.len() - self.width as usize);

            top_row.swap_with_slice(bottom_row);
            data = middle;
        }
        let elapsed = now.elapsed();
        println!("Vertical-Flip Finished in: {:?}", elapsed);
    }

    fn swap_width_height(&mut self) {
        swap(&mut self.width, &mut self.height)
    }

    fn is_square(&self) -> bool {
        self.width == self.height
    }
    fn is_large(&self) -> bool {
        self.width > 512 || self.height > 512
    }
}
