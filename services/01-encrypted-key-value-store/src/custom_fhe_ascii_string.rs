use std::fmt::{Debug, Formatter};
use std::ops::{BitAnd, Not};
use rayon::prelude::*;
use tfhe::prelude::{FheDecrypt, FheEncrypt, FheEq, IfThenElse};
use tfhe::{ClientKey, FheBool, FheUint8};

#[derive(Clone)]
pub struct CustomFheAsciiString {
    pub string: Vec<FheUint8>,
}

impl From<&Vec<u8>> for CustomFheAsciiString {
    fn from(string: &Vec<u8>) -> Self {
        let serialized_string = SerializedCustomFheAsciiString::from(string);
        serialized_string.deserialize()
    }
}

#[derive(Clone)]
pub struct SerializedCustomFheAsciiString {
    pub string: Vec<u8>,
}

impl SerializedCustomFheAsciiString {
    fn deserialize(&self) -> CustomFheAsciiString {
        let string = bincode::deserialize(&self.string).unwrap();
        CustomFheAsciiString { string }
    }
}

impl From<&Vec<u8>> for SerializedCustomFheAsciiString {
    fn from(string: &Vec<u8>) -> Self {
        Self {
            string: string.clone(),
        }
    }
}

impl CustomFheAsciiString {
    pub fn new(str: &str, client_key: &ClientKey) -> CustomFheAsciiString {
        let string = str
            .bytes()
            .map(|char| FheUint8::encrypt(char,client_key))
            .collect();
        CustomFheAsciiString { string }
    }
    pub fn serialize(&self) -> SerializedCustomFheAsciiString {
        let string = bincode::serialize(&self.string).unwrap();
        SerializedCustomFheAsciiString { string }
    }
}

impl From<&SerializedCustomFheAsciiString> for CustomFheAsciiString {
    fn from(string: &SerializedCustomFheAsciiString) -> Self {
        string.deserialize()
    }
}

// TODO() finds out if threading is faster or nah
impl FheEq for CustomFheAsciiString {
    fn eq(&self, other: Self) -> FheBool {
        assert_eq!(self.string.len(), other.string.len(), "Key length mismatch. ");
        self.string
            .iter()
            .zip(other.string.iter())
            .map(|(a, b)| a.eq(b))
            .reduce(|acc, x| acc.bitand(x))
            .expect("Key must not be empty.")
    }

    fn ne(&self, other: Self) -> FheBool {
        self.eq(other).not()
    }
}

impl FheDecrypt<String> for CustomFheAsciiString {
    fn decrypt(&self, key: &ClientKey) -> String {
        let bytes = self.string
            .iter()
            .map(|char| char.decrypt(key))
            .collect::<Vec<u8>>();

        String::from_utf8(bytes).expect("Invalid UTF-8")
    }
}

impl IfThenElse<CustomFheAsciiString> for FheBool {
    // TODO() find out if threading is faster or nah
    fn if_then_else(&self, ct_then: &CustomFheAsciiString, ct_else: &CustomFheAsciiString) -> CustomFheAsciiString {
        assert_eq!(ct_then.string.len(), ct_else.string.len(), "Key length mismatch");

        let constructed_key = ct_then.string
            .iter()
            .zip(ct_else.string.iter())
            .map(|(a, b)| self.if_then_else(a, b))
            .collect::<Vec<FheUint8>>();

        CustomFheAsciiString { string: constructed_key }
    }
}
