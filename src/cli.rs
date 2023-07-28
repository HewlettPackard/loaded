use crate::stream::checksum::Checksum;
use clap::{Args, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use std::error::Error;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

#[derive(Parser, Debug)]
#[command(
author,
version,
about = r#"
██╗░░░░░░█████╗░░█████╗░██████╗░███████╗██████╗░
██║░░░░░██╔══██╗██╔══██╗██╔══██╗██╔════╝██╔══██╗
██║░░░░░██║░░██║███████║██║░░██║█████╗░░██║░░██║
██║░░░░░██║░░██║██╔══██║██║░░██║██╔══╝░░██║░░██║
███████╗╚█████╔╝██║░░██║██████╔╝███████╗██████╔╝
╚══════╝░╚════╝░╚═╝░░╚═╝╚═════╝░╚══════╝╚═════╝░

A tool to generate http/s traffic to a server 'til it's absolutely loaded"#,
long_about = None)]
pub struct Loaded {
    #[command(subcommand)]
    pub loaded: LoadedCmd,
}

#[derive(Subcommand, Debug)]
#[command(infer_subcommands = true)]
pub enum LoadedCmd {
    /// Run an engine to generate http traffic to a server
    Run(RunCmd),
    /// Generate shell completions
    GenCompletions {
        /// Set the shell for generating completions
        #[arg(long, short)]
        shell: Shell,

        /// Set the output directory
        #[arg(long, short)]
        out_dir: Option<String>,
    },
}

#[derive(Debug, Args)]
pub struct RunCmd {
    /// URL to generate load on
    ///
    /// For example:
    /// `"http://localhost:9000/endpoint"`
    #[arg(short, long)]
    pub url: String,

    /// Format to output results
    #[arg(short, long, value_enum, default_value_t = FormatType::Pretty)]
    pub format: FormatType,

    /// Number of threads to use to generate load (defaults to number of physical cores)
    #[arg(short, long, default_value_t = num_cpus::get_physical())]
    pub threads: usize,

    /// The number of connections to open and send requests over
    #[arg(short, long, default_value_t = 1)]
    pub connections: usize,

    /// Limits the number of requests per second
    #[arg(short, long)]
    pub rate_limit: Option<u32>,

    /// Completes the run once the specified amount of time in seconds has elapsed
    #[arg(short, long, group = "completion", value_parser = parse_duration)]
    pub duration: Option<Duration>,

    /// Completes the run once the specified number of requests have been completed
    #[arg(short, long, group = "completion")]
    pub num_requests: Option<usize>,

    /// A seed to inject some randomness per run (defaults to generated UUIDv4).
    ///
    /// It is up to the engine to make use of this and it may or may not
    /// factor into each engine.
    #[arg(short, long, default_value_t = uuid::Uuid::new_v4().to_string())]
    pub seed: String,

    /// Engine to use to generate load
    #[command(subcommand)]
    pub engine: Engine,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum FormatType {
    Pretty,
    Json,
}

fn parse_duration(arg: &str) -> Result<Duration, std::num::ParseIntError> {
    let seconds = arg.parse()?;
    Ok(Duration::from_secs(seconds))
}

#[derive(Debug, Clone, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct SimpleArgs {
    /// The HTTP method for the request
    #[arg(short, long)]
    pub method: String,

    /// The HTTP headers for the request
    ///
    /// These can be specified as either as a single ',' separated string of key=value pairs:
    ///
    ///   loaded run --url <URL> simple --method <METHOD> -X"ContentType=application/json,Content-Length=500"
    ///
    /// or as a series of args:
    ///
    ///   loaded run --url <URL> simple --method <METHOD> -X"ContentType=application/json" -X"Content-Length=500"
    #[arg(short = 'X', long, value_parser = parse_key_val::< String, String >)]
    pub headers: Vec<(String, String)>,

    /// The body of the http request
    #[arg(long, group = "b")]
    pub body: Option<String>,

    /// The body of the http request, read in from the provided file
    #[arg(long, group = "b")]
    pub body_from_file: Option<PathBuf>,
}

/// Parse a single key-value pair
///
/// From clap example: <https://github.com/clap-rs/clap/blob/master/examples/typed-derive.rs>
fn parse_key_val<T, U>(s: &str) -> Result<(T, U), Box<dyn Error + Send + Sync + 'static>>
where
    T: FromStr,
    T::Err: Error + Send + Sync + 'static,
    U: FromStr,
    U::Err: Error + Send + Sync + 'static,
{
    let pos = s
        .find('=')
        .ok_or_else(|| format!("invalid KEY=value: no `=` found in `{s}`"))?;
    Ok((s[..pos].parse()?, s[pos + 1..].parse()?))
}

#[derive(Debug, Clone, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct S3Args {
    /// The bucket to operate on for our S3 requests
    #[arg(long, short)]
    pub bucket: String,

    /// The size in bytes of the object for a PUT/GET operation
    #[arg(long, short)]
    pub object_size: usize,

    /// A prefix to prepend to each object key (defaults to generated UUIDv4)
    #[arg(long,default_value_t = Uuid::new_v4().to_string())]
    pub obj_prefix: String,

    #[arg(long, short, value_enum, default_value_t = TrafficPattern::Put)]
    pub traffic_pattern: TrafficPattern,

    /// Specifies the folder depth that will be used to generate prefixes
    ///
    /// To illustrate, let's say we have an object with the name 'foo':
    /// - A folder depth of 0 results in an object with no prefix. (e.g '/foo')
    /// - A folder depth of 1 results in an object with a prefix with one "folder". (e.g. '/0/foo')
    /// - A folder depth of 2 results in an object with a prefix with one "folder". (e.g. '/0/0/foo')
    /// - So on and so forth
    #[arg(long = "folder_depth", default_value_t = 0)]
    pub prefix_folder_depth: usize,

    /// Specifies the number of objects we will generate in a given folder prefix
    ///
    /// To illustrate, let's say we have an object with the name 'foo', a prefix_folder_depth
    /// of 1 and num_branches_per_folder_depth of 1:
    /// - With num_objs_per_prefix_folder set to 1, the first three generated paths would look like:
    ///     - /0/foo0
    ///     - /0/foo0
    ///     - /0/foo0
    /// - With num_objs_per_prefix_folder set to 2, the first three generated paths would look like:
    ///     - /0/foo0
    ///     - /0/foo1
    ///     - /0/foo0
    /// - With num_objs_per_prefix_folder set to 3, the first three generated paths would look like:
    ///     - /0/foo0
    ///     - /0/foo1
    ///     - /0/foo2
    #[arg(long, default_value_t = 10000)]
    pub num_objs_per_prefix_folder: usize,

    /// Specifies the number of child folders we will generate in a given folder prefix
    ///
    /// To illustrate, let's say we have an object with the name 'foo', a prefix_folder_depth
    /// of 1 and num_branches_per_folder_depth of 1:
    /// - With num_branches_per_folder_depth set to 1, the first three generated paths would look like:
    ///     - /0/foo0
    ///     - /0/foo0
    ///     - /0/foo0
    /// - With num_branches_per_folder_depth set to 2, the first three generated paths would look like:
    ///     - /0/foo0
    ///     - /1/foo1
    ///     - /0/foo0
    /// - With num_branches_per_folder_depth set to 3, the first three generated paths would look like:
    ///     - /0/foo0
    ///     - /1/foo1
    ///     - /2/foo2
    #[arg(long = "folder_branches", default_value_t = 10)]
    pub num_branches_per_folder_depth: usize,

    /// The checksum algorithm to calculate and use for the S3 request
    #[arg(long, short)]
    pub checksum_algorithm: Option<Checksum>,
}

#[derive(Debug, Clone, Subcommand)]
#[command(subcommand_help_heading = "Engines", subcommand_value_name = "ENGINE")]
pub enum Engine {
    /// An engine for load testing a single request variant ad nauseam
    ///
    /// Note: does not make use of the `seed` argument.
    #[command(arg_required_else_help = true)]
    Simple(SimpleArgs),
    /// An engine for load testing an S3 server
    ///
    /// Note: makes use of the `seed` argument.
    #[command(arg_required_else_help = true)]
    S3(S3Args),
}

#[derive(Debug, Clone)]
pub enum CompletionCondition {
    NumRequests(usize, Arc<AtomicUsize>),
    Duration(Duration),
}

#[derive(Debug, Clone, ValueEnum)]
pub enum TrafficPattern {
    Put,
    Get,
    Both,
}
