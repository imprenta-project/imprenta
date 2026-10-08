export type Align = 'left' | 'center' | 'right';
export type Codepage = 'cp437' | 'cp850' | 'cp858' | 'cp1252';
export interface PrinterProfile {
  columns?: number;
  printableWidth?: number;
  codepage?: Codepage;
  /** ESC t index on the actual device; defaults to the Epson mapping. */
  codepageId?: number;
  dpi?: number;
}
export interface ResolvedProfile {
  columns: number;
  printableWidth: number;
  codepage: Codepage;
  codepageId: number;
  dpi: number;
  marginLeft: number;
}
export interface TextStyle {
  justification?: Align;
  font?: 'A' | 'B';
  fontWidth?: number;
  fontHeight?: number;
  bold?: boolean;
  underline?: 'none' | 'one-dot-thick' | 'two-dot-thick';
  inverse?: boolean;
  doubleStrike?: boolean;
  lineSpacing?: number;
}
export interface Init {
  type: 'init';
  width?: number;
  marginLeft?: number;
  dpi?: number;
}
export interface Text extends TextStyle {
  type: 'text';
  text: string;
  lf?: boolean;
  prefix?: string;
  suffix?: string;
  format?: string;
}
export interface Column {
  text: string;
  width?: number;
  justification?: Align;
  overflow?: 'wrap' | 'error';
}
export interface Row extends TextStyle {
  type: 'row';
  columns: Column[];
  gap?: number;
}
export interface Rule extends TextStyle {
  type: 'rule';
  character?: string;
}
export interface QRCode {
  type: 'qrcode';
  value: string;
  justification?: Align;
  model?: 1 | 2;
  size?: number;
  errorCorrectionLevel?: 'L' | 'M' | 'Q' | 'H';
}
export interface Barcode {
  type: 'barcode';
  value: string;
  justification?: Align;
  barcodeType?: 'upca' | 'upce' | 'ean13' | 'ean8' | 'code39' | 'itf' | 'code128';
  width?: number;
  height?: number;
  hriPosition?: 'not-printed' | 'above-bar-code' | 'below-bar-code' | 'above-and-below-bar-code';
  hriFont?: 'A' | 'B';
}
export interface PDF417 {
  type: 'pdf417';
  value: string;
  justification?: Align;
  width?: number;
  height?: number;
  numberOfColumns?: number;
  numberOfRows?: number;
  errorCorrectionLevel?: number;
  option?: 'standard' | 'truncated';
}
export interface Image {
  type: 'image';
  src: string;
  width: number;
  height?: number;
  justification?: Align;
  filter?: 'dither' | 'monochrome';
  threshold?: number;
}
export interface Feed {
  type: 'feed';
  lines?: number;
}
export interface Cut {
  type: 'cut';
  mode?: 'full' | 'partial';
}
export interface Bytes {
  type: 'bytes';
  hex: string;
}
export type ReceiptNode =
  | Init
  | Text
  | Row
  | Rule
  | QRCode
  | Barcode
  | PDF417
  | Image
  | Feed
  | Cut
  | Bytes;
export interface Receipt {
  profile?: PrinterProfile;
  children: ReceiptNode[];
}
export interface ImageAsset {
  name: string;
  data: Uint8Array;
}
export interface RenderOptions {
  profile?: PrinterProfile;
  images?: ImageAsset[];
  /** Worker count. Chosen when the Node pool starts; defaults to two. */
  size?: number;
  wasm?: Uint8Array<ArrayBuffer> | ArrayBuffer;
}
export interface RenderResult {
  escpos: Uint8Array;
  bytes: number;
  tickets: number;
  profile: ResolvedProfile;
  diagnostics: string[];
}
