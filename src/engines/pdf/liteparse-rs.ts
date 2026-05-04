import { execFile } from "node:child_process";
import { promisify } from "node:util";
import fs from "node:fs/promises";
import { statSync } from "node:fs";
import path from "node:path";
import os from "node:os";
import { PdfEngine, PdfDocument, PageData, Image } from "./interface.js";
import { TextItem, ParsedPage } from "../../core/types.js";

const execFileAsync = promisify(execFile);

/** JSON output shape from the Rust CLI for a single extracted page */
interface RustPageOutput {
  page_number: number;
  page_width: number;
  page_height: number;
  text_items: Array<{
    text: string;
    x: number;
    y: number;
    width: number;
    height: number;
    font_name: string | null;
    font_size: number | null;
    rotation: number;
  }>;
}

/** JSON output shape from the Rust CLI `parse` command (extraction + projection) */
interface RustParsedPageOutput extends RustPageOutput {
  text: string;
}

/** Extended PdfDocument that stores the file path for CLI invocations */
interface RustPdfDocument extends PdfDocument {
  _filePath: string;
  /** Cached extraction results from loadDocument (to avoid re-running CLI) */
  _cachedPages?: RustPageOutput[];
}

/**
 * Resolve the path to the liteparse-rs binary.
 *
 * Priority:
 * 1. LITEPARSE_RS_BIN environment variable
 * 2. Release build at packages/rust/target/release/liteparse-rs
 * 3. Debug build at packages/rust/target/debug/liteparse-rs
 */
function resolveBinaryPath(): string {
  if (process.env.LITEPARSE_RS_BIN) {
    return process.env.LITEPARSE_RS_BIN;
  }

  const rustPkgDir = path.resolve(
    new URL(import.meta.url).pathname,
    "../../../../packages/rust"
  );
  const releaseBin = path.join(rustPkgDir, "target/release/liteparse-rs");
  const debugBin = path.join(rustPkgDir, "target/debug/liteparse-rs");

  // Prefer release, fall back to debug
  try {
    if (statSync(releaseBin).isFile()) return releaseBin;
  } catch { /* not found */ }
  try {
    if (statSync(debugBin).isFile()) return debugBin;
  } catch { /* not found */ }

  return releaseBin; // default — will surface a clear error on exec
}

const BINARY_PATH = resolveBinaryPath();

export class LiteParseRsEngine implements PdfEngine {
  name = "liteparse-rs";

  async loadDocument(input: string | Uint8Array, _password?: string): Promise<PdfDocument> {
    let filePath: string;

    if (typeof input === "string") {
      filePath = input;
    } else {
      // Write buffer to a temp file since the Rust CLI only accepts file paths
      const tempFile = path.join(os.tmpdir(), `liteparse-rs-${Date.now()}.pdf`);
      await fs.writeFile(tempFile, input);
      filePath = tempFile;
    }

    // Run a full extraction to discover page count (and cache results)
    const pages = await this.runExtract(filePath);

    // Read the raw bytes so the PdfDocument contract is satisfied
    const data = new Uint8Array(await fs.readFile(filePath));

    return {
      numPages: pages.length,
      data,
      _filePath: filePath,
      _cachedPages: pages,
    } as RustPdfDocument;
  }

  async extractPage(doc: PdfDocument, pageNum: number): Promise<PageData> {
    const rustDoc = doc as RustPdfDocument;

    // Use cached data if available
    const cached = rustDoc._cachedPages?.find((p) => p.page_number === pageNum);
    if (cached) {
      return this.convertPage(cached);
    }

    // Otherwise run CLI for just this page
    const pages = await this.runExtract(rustDoc._filePath, pageNum);
    if (pages.length === 0) {
      return { pageNum, width: 0, height: 0, textItems: [], images: [] };
    }
    return this.convertPage(pages[0]);
  }

  async extractAllPages(
    doc: PdfDocument,
    maxPages?: number,
    targetPages?: string
  ): Promise<PageData[]> {
    const rustDoc = doc as RustPdfDocument;

    // Use cached data from loadDocument when possible
    let rawPages = rustDoc._cachedPages;
    if (!rawPages) {
      rawPages = await this.runExtract(rustDoc._filePath);
    }
    // Clear cache after first use to free memory
    rustDoc._cachedPages = undefined;

    // Filter to target pages if specified
    if (targetPages) {
      const targetSet = this.parseTargetPages(targetPages, doc.numPages);
      rawPages = rawPages.filter((p) => targetSet.has(p.page_number));
    }

    // Apply maxPages limit
    if (maxPages && rawPages.length > maxPages) {
      rawPages = rawPages.slice(0, maxPages);
    }

    return rawPages.map((p) => this.convertPage(p));
  }

  async renderPageImage(
    _doc: PdfDocument,
    _pageNum: number,
    _dpi: number,
    _password?: string
  ): Promise<Buffer> {
    // TODO: implement page rendering in the Rust CLI
    throw new Error("renderPageImage is not yet implemented in liteparse-rs");
  }

  async close(_doc: PdfDocument): Promise<void> {
    // Nothing to clean up — each CLI invocation is stateless
  }

  /**
   * Run the full Rust pipeline (extract + grid projection) and return ParsedPages directly.
   * This skips the TypeScript grid projection entirely.
   */
  async parseDocument(
    doc: PdfDocument,
    maxPages?: number,
    targetPages?: string
  ): Promise<ParsedPage[]> {
    const rustDoc = doc as RustPdfDocument;
    let parsedPages = await this.runParse(rustDoc._filePath);

    // Filter to target pages if specified
    if (targetPages) {
      const targetSet = this.parseTargetPages(targetPages, doc.numPages);
      parsedPages = parsedPages.filter((p) => targetSet.has(p.page_number));
    }

    // Apply maxPages limit
    if (maxPages && parsedPages.length > maxPages) {
      parsedPages = parsedPages.slice(0, maxPages);
    }

    return parsedPages.map((p) => this.convertParsedPage(p));
  }

  // ── private helpers ──────────────────────────────────────────────

  /** Run the Rust CLI and parse JSON-line output */
  private async runExtract(filePath: string, pageNum?: number): Promise<RustPageOutput[]> {
    const args = ["extract", "--pdf-path", filePath];
    if (pageNum !== undefined) {
      args.push("--page-num", String(pageNum));
    }

    let stdout: string;
    try {
      const result = await execFileAsync(BINARY_PATH, args, {
        maxBuffer: 100 * 1024 * 1024, // 100 MB
      });
      stdout = result.stdout;
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : String(err);
      throw new Error(`liteparse-rs failed: ${message}`);
    }

    // Each line is a JSON object for one page
    const pages: RustPageOutput[] = [];
    for (const line of stdout.split("\n")) {
      const trimmed = line.trim();
      if (!trimmed) continue;
      pages.push(JSON.parse(trimmed) as RustPageOutput);
    }

    return pages;
  }

  /** Run the Rust CLI `parse` command (extract + projection) and parse JSON output */
  private async runParse(filePath: string): Promise<RustParsedPageOutput[]> {
    const args = ["parse", "--pdf-path", filePath];

    let stdout: string;
    try {
      const result = await execFileAsync(BINARY_PATH, args, {
        maxBuffer: 100 * 1024 * 1024, // 100 MB
      });
      stdout = result.stdout;
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : String(err);
      throw new Error(`liteparse-rs parse failed: ${message}`);
    }

    // `parse` outputs a single JSON array
    return JSON.parse(stdout) as RustParsedPageOutput[];
  }

  /** Convert Rust CLI parsed page output to ParsedPage format */
  private convertParsedPage(raw: RustParsedPageOutput): ParsedPage {
    const pageHeight = raw.page_height;

    const textItems: TextItem[] = raw.text_items.map((item) => {
      // pdfium uses bottom-left origin; LiteParse expects top-left origin.
      // Convert: y_top = page_height - y_bottom - item_height
      const y = pageHeight - item.y - item.height;

      return {
        str: item.text,
        x: item.x,
        y,
        width: item.width,
        height: item.height,
        w: item.width,
        h: item.height,
        fontName: item.font_name ?? undefined,
        fontSize: item.font_size ?? undefined,
        r: item.rotation,
      };
    });

    return {
      pageNum: raw.page_number,
      width: raw.page_width,
      height: raw.page_height,
      text: raw.text,
      textItems,
    };
  }

  /** Convert Rust CLI output to the PageData format */
  private convertPage(raw: RustPageOutput): PageData {
    const pageWidth = raw.page_width;
    const pageHeight = raw.page_height;

    const textItems: TextItem[] = raw.text_items.map((item) => {
      // pdfium uses bottom-left origin; LiteParse expects top-left origin.
      // Convert: y_top = page_height - y_bottom - item_height
      const y = pageHeight - item.y - item.height;

      return {
        str: item.text,
        x: item.x,
        y,
        width: item.width,
        height: item.height,
        w: item.width,
        h: item.height,
        fontName: item.font_name ?? undefined,
        fontSize: item.font_size ?? undefined,
        r: item.rotation,
      };
    });

    const images: Image[] = [];

    return {
      pageNum: raw.page_number,
      width: pageWidth,
      height: pageHeight,
      textItems,
      images,
    };
  }

  private parseTargetPages(targetPages: string, maxPages: number): Set<number> {
    const pages = new Set<number>();
    for (const part of targetPages.split(",")) {
      const trimmed = part.trim();
      if (trimmed.includes("-")) {
        const [start, end] = trimmed.split("-").map((n) => parseInt(n.trim()));
        for (let i = start; i <= Math.min(end, maxPages); i++) {
          if (i >= 1) pages.add(i);
        }
      } else {
        const num = parseInt(trimmed);
        if (num >= 1 && num <= maxPages) pages.add(num);
      }
    }
    return pages;
  }
}
