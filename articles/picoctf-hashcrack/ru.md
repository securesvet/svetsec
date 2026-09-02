---
title: "PicoCTF: hashcrack"
date: 2025-04-18
labels:
  - cryptography
  - ctf
  - python
---

# PicoCTF: hashcrack

Задание сложности `easy` на
[picoctf.org](https://play.picoctf.org/practice/challenge/475?category=2&difficulty=1&page=1)
показывает, что такое хэши и почему слабые пароли нельзя защищать одним быстрым
хэшированием. Давайте посмотрим на него подробнее.

![Условие задания](assets/picoctf-hashcrack-task.png)

> A company stored a secret message on a server which got breached due to the
> admin using weakly hashed passwords. Can you gain access to the secret stored
> within the server?
>
> Access the server using `nc verbal-sleep.picoctf.net 57192`

Подключаемся:

```shell
nc verbal-sleep.picoctf.net 57192
```

Сервер показывает первый хэш:

```text
Welcome!! Looking For the Secret?
We have identified a hash: 482c811da5d5b4bc6d497ffa98491e38
Enter the password for identified hash:
```

Для начала нужно понять, какой алгоритм мог создать строку такой длины.

| Алгоритм | Длина в hex | Пример                                                           |
| -------- | ------------ | ---------------------------------------------------------------- |
| MD5      | 32 символа   | 5f4dcc3b5aa765d61d8327deb882cf99                                 |
| SHA-1    | 40 символов  | b7a875fc1ea228b9061041b7cec4bd3c52ab3ce3                         |
| SHA-256  | 64 символа   | cd0894152aa5eec36ec79eb2bcb90ca40f056804530f40732b4957a496b23dc8 |

У первого значения 32 символа:

```python
print(len("482c811da5d5b4bc6d497ffa98491e38"))
```

Это хороший признак MD5. В условии также спрятана подсказка: администратор
использовал слабые пароли. Проверим популярные пароли из `rockyou.txt`: для
каждой строки вычислим MD5 и сравним с полученным значением.

![Скрипт перебора](assets/picoctf-hashcrack-script.png)

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

Результат:

```text
WE GOT A HIT
password123
```

После ввода пароля сервер выдаёт следующий хэш:

```text
b7a875fc1ea228b9061041b7cec4bd3c52ab3ce3
```

В нём 40 символов, поэтому повторяем проверку с SHA-1:

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

Последний хэш заметно длиннее:

```text
916e8c4f79b25028c9e467f1eb8eee6d6bbdff965f9928310ad30a8d88697745
```

64 шестнадцатеричных символа указывают на SHA-256:

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

После третьего ответа сервер показывает флаг. Мы нашли три типа хэшей по длине
и проверили словарь популярных паролей подходящим алгоритмом.

Главный практический вывод: быстрый MD5, SHA-1 или SHA-256 без соли не превращает
слабый пароль в сильный. Для хранения паролей нужны специально предназначенные
алгоритмы вроде Argon2id, уникальная соль и подходящие параметры стоимости.
