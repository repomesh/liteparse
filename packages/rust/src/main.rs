use clap::{Parser, Subcommand, Args};
use liteparse_rs::extract;
use liteparse_rs::projection;


#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Extract raw text items from a PDF file (no grid projection)
    Extract(PdfCommand),
    /// Parse a PDF file: extract + grid projection, output projected pages as JSON
    Parse(PdfCommand),
}


#[derive(Args, Debug)]
struct PdfCommand {
    /// Specify the path to the PDF file
    #[arg(long)]
    pdf_path: String,

    /// Optionally specify a target page number
    #[arg(long)]
    page_num: Option<u32>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Extract(cmd) => {
            extract::extract(&cmd.pdf_path, cmd.page_num)?;
        }
        Commands::Parse(cmd) => {
            let pages = extract::extract_pages(&cmd.pdf_path, cmd.page_num)?;
            let parsed_pages = projection::project_pages_to_grid(pages);
            // Output all parsed pages as a single JSON array
            println!("{}", serde_json::to_string(&parsed_pages)?);
        }
    }

    Ok(())
}
