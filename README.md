![](./loaded.svg)

A tool to generate http/s traffic to a server 'til it's absolutely loaded.

## Usage

```shell
$ loaded -h
██╗░░░░░░█████╗░░█████╗░██████╗░███████╗██████╗░
██║░░░░░██╔══██╗██╔══██╗██╔══██╗██╔════╝██╔══██╗
██║░░░░░██║░░██║███████║██║░░██║█████╗░░██║░░██║
██║░░░░░██║░░██║██╔══██║██║░░██║██╔══╝░░██║░░██║
███████╗╚█████╔╝██║░░██║██████╔╝███████╗██████╔╝
╚══════╝░╚════╝░╚═╝░░╚═╝╚═════╝░╚══════╝╚═════╝░

A tool to generate http/s traffic to a server 'til it's absolutely loaded

Usage: loaded <COMMAND>

Commands:
  run              Run an engine to generate http traffic to a server
  gen-completions  Generate shell completions
  help             Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

### Run

The main entry point to loaded is the CLI command `loaded run` that allows you to specify a engine for the workload and options related to the workload:

```shell
$ loaded run -h
Run an engine to generate http traffic to a server

Usage: loaded run [OPTIONS] --url <URL> <ENGINE>

Engines:
  simple  An engine for load testing a single request variant ad nauseam
  s3      An engine for load testing an S3 server
  help    Print this message or the help of the given subcommand(s)

Options:
  -u, --url <URL>                    URL to generate load on
  -f, --format <FORMAT>              Format to output results [default: pretty] [possible values: pretty, json]
  -t, --threads <THREADS>            Number of threads to use to generate load (defaults to number of physical cores) [default: 10]
  -c, --connections <CONNECTIONS>    The number of connections to open and send requests over [default: 1]
  -r, --rate-limit <RATE_LIMIT>      Limits the number of requests per second
  -d, --duration <DURATION>          Completes the run once the specified amount of time in seconds has elapsed
  -n, --num-requests <NUM_REQUESTS>  Completes the run once the specified number of requests have been completed
  -s, --seed <SEED>                  A seed to inject some randomness per run (defaults to generated UUIDv4) [default: 973a8321-fe29-4678-aeb7-7ca04539fe37]
  -h, --help                         Print help (see more with '--help')
```

#### Simple Engine

```shell
$ loaded run simple -h
An engine for load testing a single request variant ad nauseam

Usage: loaded run --url <URL> simple [OPTIONS] --method <METHOD>

Options:
  -m, --method <METHOD>
          The HTTP method for the request
  -X, --headers <HEADERS>
          The HTTP headers for the request
      --body <BODY>
          The body of the http request
      --body-from-file <BODY_FROM_FILE>
          The body of the http request, read in from the provided file
  -h, --help
          Print help (see more with '--help')
```

#### S3 Engine

```shell
An engine for load testing an S3 server

Usage: loaded run --url <URL> s3 [OPTIONS] --bucket <BUCKET> --object-size <OBJECT_SIZE>

Options:
  -b, --bucket <BUCKET>
          The bucket to operate on for our S3 requests
  -o, --object-size <OBJECT_SIZE>
          The size in bytes of the object for a PUT/GET operation
      --obj-prefix <OBJ_PREFIX>
          A prefix to prepend to each object key (defaults to generated UUIDv4) [default: f56c0581-d3e0-4192-9ad1-4a519c4d522b]
  -t, --traffic-pattern <TRAFFIC_PATTERN>
          [default: put] [possible values: put, get, both]
      --folder_depth <PREFIX_FOLDER_DEPTH>
          Specifies the folder depth that will be used to generate prefixes [default: 0]
      --num-objs-per-prefix-folder <NUM_OBJS_PER_PREFIX_FOLDER>
          Specifies the number of objects we will generate in a given folder prefix [default: 10000]
      --folder_branches <NUM_BRANCHES_PER_FOLDER_DEPTH>
          Specifies the number of child folders we will generate in a given folder prefix [default: 10]
  -c, --checksum-algorithm <CHECKSUM_ALGORITHM>
          The checksum algorithm to calculate and use for the S3 request
  -h, --help
          Print help (see more with '--help')
```

### Generate Shell Completions

loaded has support for generating tab-completion for various shells:

```shell
$ loaded gen-completions -h
Generate shell completions

Usage: loaded gen-completions [OPTIONS] --shell <SHELL>

Options:
  -s, --shell <SHELL>      Set the shell for generating completions [possible values: bash, elvish, fish, powershell, zsh]
  -o, --out-dir <OUT_DIR>  Set the output directory
  -h, --help               Print help
```

It's as simple as using one of the following (glommed from rustup):

```shell
# Bash
$ loaded gen-completions --shell bash > ~/.local/share/bash-completion/completions/loaded

# Bash (macOS/Homebrew)
$ loaded gen-completions --shell bash > $(brew --prefix)/etc/bash_completion.d/loaded.bash-completion

# Fish
$ mkdir -p ~/.config/fish/completions
$ loaded gen-completions --shell fish > ~/.config/fish/completions/loaded.fish

# Zsh
$ loaded gen-completions --shell zsh > ~/.zfunc/_rustup
```

**Note**: you may need to restart your shell in order for the changes to take effect.

For `zsh`, you must then add the following line in your `~/.zshrc` before
`compinit`:

```zsh
fpath+=~/.zfunc
```

## Building

loaded is written in Rust, so you'll need install a Rust Compiler and your best bet is to use [rustup](https://rustup.rs/). loaded compiles with Rust 1.71.0 (stable) or newer.

To build loaded:

```shell
$ git clone https://github.hpe.com/hpe/loaded
$ cd loaded
$ cargo build --release
$ ./target/release/loaded --version
loaded 0.1.0
```