---
labels:
  - cryptography
  - ctf
  - rust
---

# PicoCTF: Vigenere

Шифр Виженера похож на шифр Цезаря, который также встречается под названиями
ROT-13, ROT-14 и так далее. Главное отличие в том, что смещение задаётся не
одним числом, а повторяющимся ключом.

Например, зашифруем слово «СЛОН» ключом «КЛЮЧ». Для каждой буквы текста берём
позицию соответствующей буквы ключа в алфавите и складываем позиции по модулю
длины алфавита. После последней буквы ключ начинается заново.

- В русском алфавите с `Ё`, если считать от нуля, `С = 18`, а `К = 11`.
  Поэтому первая зашифрованная позиция равна `(18 + 11) mod 33 = 29`, то есть
  букве `Ь`.
- Затем по той же схеме обрабатываем оставшиеся буквы текста и ключа.

В задании нам дают файл `cipher.txt` и ключ `CYLAB`, которым был зашифрован
текст:

```txt
rgnoDVD{O0NU_WQ3_G1G3O3T3_A1AH3S_2951c89f}
```

Для решения хватило бы короткого скрипта на Python, который сдвигает буквы в
ASCII согласно ключу. Но для разнообразия напишем программу на Rust:

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

Получаем ожидаемый результат с полезным советом и «солью»:

`picoCTF{D0NT_US3_V1G3N3R3_C1PH3R_2951a89h}`
