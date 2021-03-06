use crate::blowfish;
use crate::rsa;
use crate::rsatools;
use crate::delivery::{push_value, pop_value, push_slice};
use crate::read_file;

pub type ResultVec = Result<Vec<u8>, &'static str>;

pub trait Encryption : Send + Sync {
    fn encrypt(&self, v: &Vec<u8>) -> ResultVec;
    fn decrypt(&self, v: &Vec<u8>) -> ResultVec;
    fn encryption_key(&self) -> Vec<u8>;
}

pub struct SymmetricEncryption {
    algorithm: blowfish::Blowfish
}

pub struct AsymmetricEncryption {
    pub_key: String,
    priv_key: String
}

// ---------------------------------

impl SymmetricEncryption {

    pub fn new(hexkey: &String) -> Result<SymmetricEncryption, &'static str> {

        Ok(SymmetricEncryption {
            algorithm: blowfish::Blowfish::from_key(from_hex(hexkey.clone())?)?
        })
    }
}

impl Encryption for SymmetricEncryption {

    /// Encrypts the given data stored in a vector and returns the concatenated
    /// IV and ciphertext.
    fn encrypt(&self, v: &Vec<u8>) -> ResultVec {
        self.algorithm.encrypt(v)
    }

    /// Decrypts the given daa stored in a vector and returns the plaintext.
    fn decrypt(&self, v: &Vec<u8>) -> ResultVec {
        self.algorithm.decrypt(v)
    }

    /// Returns the symmetric key used for encryption and decryption.
    fn encryption_key(&self) -> Vec<u8> {
        self.algorithm.key()
    }
}

// ---------------------------------

impl AsymmetricEncryption {

    pub fn new(pubkey_file: &str, privkey_file: &str) -> Result<AsymmetricEncryption, &'static str> {

        Ok(AsymmetricEncryption {
            pub_key: read_file(pubkey_file)?,
            priv_key: read_file(privkey_file)?
        })
    }
}

// ---------------------------------

impl Encryption for AsymmetricEncryption {

    fn encrypt(&self, v: &Vec<u8>) -> ResultVec {

        // Encrypt the data with Blowfish.
        let symenc = blowfish::Blowfish::new()?;
        let cipher = symenc.encrypt(v)?;

        // Encrypt the key used by Blowfish with RSA.
        let ekey =
            rsa::RSA::new(&self.pub_key, &self.priv_key)?.encrypt(&symenc.key())?;

        let mut v: Vec<u8> = Vec::new();
        push_value(&mut v, cipher.len() as u64, 8); // length of ciphertext
        push_slice(&mut v, &cipher);                // ciphertext
        push_slice(&mut v, &ekey);                  // with RSA encrypted key
        Ok(v)
    }
 
    fn decrypt(&self, v: &Vec<u8>) -> ResultVec {

        let mut data = v.clone();
        let clen = pop_value(&mut data, 8)? as usize;

        if clen > data.len() {
            return Err("Invalid ciphertext length.");
        }

        let (cipher, cipher_key) = data.split_at(clen);


        blowfish::Blowfish::from_key(
            rsa::RSA::new(&self.pub_key, &self.priv_key)?.decrypt(cipher_key)?
        )?.decrypt(cipher)
    }

    /// Returns the public key.
    fn encryption_key(&self) -> Vec<u8> {
        rsatools::key_as_der(&self.pub_key)
    }
}

// ------------------------------------------------------------------

pub fn from_hex(s: String) -> ResultVec {

    let bytes = s.into_bytes();

    if bytes.len() % 2 != 0 {
        return Err("Length of hexadecimal string is not a multiple of 2.");
    }

    let mut v: Vec<u8> = vec![];
    let mut p: usize = 0;
    while p < bytes.len() {
        let mut b: u8 = 0;
        for _ in 0..2 {
            b = b << 4;
            let val = bytes[p];
            match val {
                b'A'...b'F' => b += val - b'A' + 10,
                b'a'...b'f' => b += val - b'a' + 10,
                b'0'...b'9' => b += val - b'0',
                _ => { return Err("Invalid character in hexadecimal string."); }
            }
            p += 1;
        }
        v.push(b);
    }
    Ok(v)
}

// ------------------------------------------------------------------------
// TESTS
// ------------------------------------------------------------------------

#[cfg(test)]
mod tests {

    #[test]
    fn test_from_hex() {
        
        let mut r = super::from_hex("0".to_string());
        assert!(r.is_err());

        r = super::from_hex("0001090A0F10".to_string());
        assert!(r.is_ok());

        let o: Vec<u8> = vec![0, 1, 9, 10, 15, 16];
        let v = r.unwrap();
        assert!(v.len() == 6);
        assert_eq!(o, v);
    }

    // --------------------------------------------------------------
 
    use super::{Encryption, AsymmetricEncryption};

    #[test]
    fn test_asymmetric_encryption() {
        
        let a = AsymmetricEncryption::new("tests/keys/rsa_pub.pem", "tests/keys/rsa_priv.pem");
        assert!(a.is_ok());

        let b = AsymmetricEncryption::new("tests/keys/rsa_pub.pem", "abc");
        assert!(b.is_err());

    }

    #[test]
    fn test_asymmetric_encrypt_decrypt() {
        
        let a = AsymmetricEncryption::new("tests/keys/rsa_pub.pem", "tests/keys/rsa_priv.pem");
        assert!(a.is_ok());
        match a {
            Ok(a) => {
                let plain  = "hello".to_string().into_bytes();
                let cipher = a.encrypt(&plain).unwrap();
                let p      = a.decrypt(&cipher).unwrap();
                assert_eq!(plain, p);
            }
            _ => { }
        }
    }
}
