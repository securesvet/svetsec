---
labels:
  - cryptography
  - ctf
  - python
---

# PicoCTF: hashcrack

This `easy` challenge on
[picoctf.org](https://play.picoctf.org/practice/challenge/475?category=2&difficulty=1&page=1)
introduces hashes and shows why a fast hash alone cannot protect a weak
password. Let us take a closer look.

![Challenge description](assets/picoctf-hashcrack-task.png)

> A company stored a secret message on a server which got breached due to the
> admin using weakly hashed passwords. Can you gain access to the secret stored
> within the server?
>
> Access the server using `nc verbal-sleep.picoctf.net 57192`

Connect to the service:

```shell
nc verbal-sleep.picoctf.net 57192
```

The server presents the first hash:

```text
Welcome!! Looking For the Secret?
We have identified a hash: 482c811da5d5b4bc6d497ffa98491e38
Enter the password for identified hash:
```

First, we need to identify which algorithm could have produced a value of this
length.

| Algorithm | Hex length | Example                                                          |
| --------- | ---------- | ---------------------------------------------------------------- |
| MD5       | 32 chars   | 5f4dcc3b5aa765d61d8327deb882cf99                                 |
| SHA-1     | 40 chars   | b7a875fc1ea228b9061041b7cec4bd3c52ab3ce3                         |
| SHA-256   | 64 chars   | cd0894152aa5eec36ec79eb2bcb90ca40f056804530f40732b4957a496b23dc8 |

The first value contains 32 characters:

```python
print(len("482c811da5d5b4bc6d497ffa98491e38"))
```

That is a strong hint that it is MD5. The challenge description contains
another clue: the administrator used weak passwords. We can test common
passwords from `rockyou.txt`, hash every line with MD5, and compare the result
with the target.

![Dictionary-checking script](assets/picoctf-hashcrack-script.png)

```python
import hashlib

target = "482c811da5d5b4bc6d497ffa98491e38"

with open("/home/svetsec/ctf/rockyou.txt", encoding="utf-8", errors="ignore") as words:
    for password in words:
        password = password.rstrip()
        if hashlib.md5(password.encode()).hexdigest() == target:
            print("WE GOT A HIT")
            print(password)
            break
```

The result is:

```text
WE GOT A HIT
password123
```

After submitting that password, the server returns another hash:

```text
b7a875fc1ea228b9061041b7cec4bd3c52ab3ce3
```

It contains 40 characters, so we repeat the check using SHA-1:

```python
import hashlib

target = "b7a875fc1ea228b9061041b7cec4bd3c52ab3ce3"

with open("/home/svetsec/ctf/rockyou.txt", encoding="utf-8", errors="ignore") as words:
    for password in words:
        password = password.rstrip()
        if hashlib.sha1(password.encode()).hexdigest() == target:
            print("WE GOT A HIT")
            print(password)
            break
```

```text
WE GOT A HIT
letmein
```

The final hash is noticeably longer:

```text
916e8c4f79b25028c9e467f1eb8eee6d6bbdff965f9928310ad30a8d88697745
```

Its 64 hexadecimal characters point to SHA-256:

```python
import hashlib

target = "916e8c4f79b25028c9e467f1eb8eee6d6bbdff965f9928310ad30a8d88697745"

with open("/home/svetsec/ctf/rockyou.txt", encoding="utf-8", errors="ignore") as words:
    for password in words:
        password = password.rstrip()
        if hashlib.sha256(password.encode()).hexdigest() == target:
            print("WE GOT A HIT")
            print(password)
            break
```

```text
WE GOT A HIT
qwerty098
```

After the third answer, the server reveals the flag. We identified three hash
types by their lengths and checked a common-password dictionary with the
corresponding algorithms.

The practical lesson is broader: fast, unsalted MD5, SHA-1, or SHA-256 does not
turn a weak password into a strong one. Password storage should use a dedicated
algorithm such as Argon2id, a unique salt, and appropriate cost parameters.
