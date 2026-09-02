---
title: "PicoCTF: Vigenere"
date: 2025-04-26
labels:
  - cryptography
  - ctf
  - rust
---

# PicoCTF: Vigenere

The Vigenère cipher resembles the Caesar cipher, which also appears under names
such as ROT-13 and ROT-14. The important difference is that Vigenère does not
use one fixed shift: its shifts come from a repeating key.

Suppose we encrypt the word `LION` with the key `KEYS`. For every plaintext
letter, we take the alphabet position of the matching key letter, add the two
positions modulo the alphabet length, and restart the key when we reach its
end.

- With zero-based positions, `L = 11` and `K = 10`, so the first encrypted
  position is `(11 + 10) mod 26 = 21`, or `V`.
- We then process the remaining plaintext and key letters in the same way.

The challenge gives us a `cipher.txt` file and the key `CYLAB`. This is the
encrypted text:

```txt
rgnoDVD{O0NU_WQ3_G1G3O3T3_A1AH3S_2951c89f}
```

A small Python script shifting ASCII letters according to the key would be
enough. For variety, however, let us write it in Rust:

```rust
use std::fs;

const ALPHABET_LENGTH: u8 = 26;

fn main() {
    let file_path = "/home/svetsec/ctf/pico/vigenere/cipher.txt";
    println!("In file {file_path}");

    let contents = fs::read_to_string(file_path)
        .expect("Should have been able to read the file");

    println!("With text:\n{contents}");

    let key = "CYLAB";
    let mut result = String::new();
    let mut added_non_alphabetical = 0;

    for (i, character) in contents.chars().enumerate() {
        if !character.is_alphabetic() {
            added_non_alphabetical += 1;
            result.push(character);
            continue;
        }

        let is_uppercase = character.is_uppercase();
        let base = if is_uppercase { b'A' } else { b'a' };
        let alphabetic_order = character.to_ascii_lowercase() as u8 - b'a';

        let shift = key
            .chars()
            .nth((i - added_non_alphabetical) % key.len())
            .unwrap()
            .to_ascii_lowercase() as u8
            - b'a';
        let shifted =
            (alphabetic_order as i8 - shift as i8).rem_euclid(ALPHABET_LENGTH as i8);

        let new_character = (shifted + base as i8) as u8 as char;
        result.push(new_character);
    }

    println!("{result}");
}
```

The result contains the expected useful advice and a little “salt”:

`picoCTF{D0NT_US3_V1G3N3R3_C1PH3R_2951a89h}`
