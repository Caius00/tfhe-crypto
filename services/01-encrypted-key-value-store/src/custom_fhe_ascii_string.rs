use std::ops::{BitAnd, Not};
use tfhe::prelude::{CiphertextList, FheDecrypt, FheEncrypt, FheEq, FheTrivialEncrypt, IfThenElse};
use tfhe::{
    ClientKey, CompactCiphertextList, CompressedCiphertextList,
    CompressedCiphertextListBuilder, FheBool, FheUint8,
};

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

#[derive(Clone)]
pub struct CompressedCustomFheAsciiString {
    pub string: Vec<u8>,
}

impl CompressedCustomFheAsciiString {
    pub fn new(compressed_string: Vec<u8>) -> Self {
        CompressedCustomFheAsciiString {
            string: compressed_string,
        }
    }
    pub fn route_decompress(&self) -> CustomFheAsciiString {
        let compact_list: CompactCiphertextList = bincode::deserialize(&self.string).unwrap();
        let expanded_list = compact_list.expand().unwrap();

        let string = (0..expanded_list.len())
            .map(|i| expanded_list.get(i).unwrap().unwrap())
            .collect::<Vec<FheUint8>>();

        CustomFheAsciiString { string }
    }

    pub fn decompress(&self) -> CustomFheAsciiString {
        let compressed_list: CompressedCiphertextList = bincode::deserialize(&self.string).unwrap();

        let string = (0..compressed_list.len())
            .map(|i| compressed_list.get(i).unwrap().unwrap())
            .collect::<Vec<FheUint8>>();

        CustomFheAsciiString { string }
    }
}

impl CustomFheAsciiString {
    pub fn new(str: &str, client_key: &ClientKey) -> CustomFheAsciiString {
        let string = str
            .bytes()
            .map(|char| FheUint8::encrypt(char, client_key))
            .collect();
        CustomFheAsciiString { string }
    }
    pub fn serialize(&self) -> SerializedCustomFheAsciiString {
        let string = bincode::serialize(&self.string).unwrap();
        SerializedCustomFheAsciiString { string }
    }

    pub fn compress(&self) -> CompressedCustomFheAsciiString {
        let compressed_list = self
            .string
            .clone()
            .into_iter()
            .fold(CompressedCiphertextListBuilder::new(), |mut builder, s| {
                builder.push(s);
                builder
            })
            .build()
            .unwrap();
        let serialized = bincode::serialize(&compressed_list).unwrap();

        CompressedCustomFheAsciiString { string: serialized }
    }
}

impl From<&SerializedCustomFheAsciiString> for CustomFheAsciiString {
    fn from(string: &SerializedCustomFheAsciiString) -> Self {
        string.deserialize()
    }
}

impl FheEq for CustomFheAsciiString {
    fn eq(&self, other: Self) -> FheBool {
        if self.string.len() != other.string.len() {
            return FheBool::encrypt_trivial(false);
        }
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
        let bytes = self
            .string
            .iter()
            .map(|char| char.decrypt(key))
            .collect::<Vec<u8>>();

        String::from_utf8(bytes).expect("Invalid UTF-8")
    }
}

impl IfThenElse<CustomFheAsciiString> for FheBool {
    fn if_then_else(
        &self,
        ct_then: &CustomFheAsciiString,
        ct_else: &CustomFheAsciiString,
    ) -> CustomFheAsciiString {
        assert_eq!(
            ct_then.string.len(),
            ct_else.string.len(),
            "Key length mismatch"
        );

        let constructed_key = ct_then
            .string
            .iter()
            .zip(ct_else.string.iter())
            .map(|(a, b)| self.if_then_else(a, b))
            .collect::<Vec<FheUint8>>();

        CustomFheAsciiString {
            string: constructed_key,
        }
    }
}

#[cfg(test)]
mod test_custom_fhe_ascii_string {
    use super::*;
    use tfhe::shortint::parameters::{Backend, Constraint, Log2PFail, MetaParametersFinder};
    use tfhe::{set_server_key, CompressedServerKey};

    #[test]
    fn eq() {
        let parameters =
            MetaParametersFinder::new(Constraint::LessThanOrEqual(Log2PFail(-128.0)), Backend::Cpu)
                .with_compression(true)
                .find()
                .expect("Could not find suitable parameters");

        let client_key = ClientKey::generate(parameters);
        let compressed_server_key = CompressedServerKey::new(&client_key);
        set_server_key(compressed_server_key.decompress());

        let str_a = "Hello World!";
        let str_b = "Hello Earth!";

        let enc_a = CustomFheAsciiString::new(str_a, &client_key);
        let enc_a_2 = CustomFheAsciiString::new(str_a, &client_key);
        let enc_b = CustomFheAsciiString::new(str_b, &client_key);

        let enc_eq = enc_a.eq(enc_a_2);
        let enc_ne = enc_a.eq(enc_b);

        assert!(enc_eq.decrypt(&client_key));
        assert!(!enc_ne.decrypt(&client_key));
    }

    #[test]
    fn compress_decompress() {
        let parameters =
            MetaParametersFinder::new(Constraint::LessThanOrEqual(Log2PFail(-128.0)), Backend::Cpu)
                .with_compression(true)
                .find()
                .expect("Could not find suitable parameters");

        let client_key = ClientKey::generate(parameters);
        let compressed_server_key = CompressedServerKey::new(&client_key);
        set_server_key(compressed_server_key.decompress());

        let str_a = "Hello World!";
        let str_b = "Hello Earth!";

        let enc_a = CustomFheAsciiString::new(str_a, &client_key);
        let enc_b = CustomFheAsciiString::new(str_b, &client_key);

        let comp_decomp_a = enc_a.compress().decompress();
        let comp_decomp_b = enc_b.compress().decompress();

        let dec_a = enc_a.decrypt(&client_key);
        let dec_b = enc_b.decrypt(&client_key);
        let dec_comp_decomp_a = comp_decomp_a.decrypt(&client_key);
        let dec_comp_decomp_b = comp_decomp_b.decrypt(&client_key);

        assert_eq!(dec_a, dec_comp_decomp_a);
        assert_eq!(dec_b, dec_comp_decomp_b);
        assert_ne!(dec_a, dec_comp_decomp_b);
        assert_ne!(dec_b, dec_comp_decomp_a);
    }
}
