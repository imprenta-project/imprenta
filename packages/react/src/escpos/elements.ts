import type {
  Barcode as IrBarcode,
  Column as IrColumn,
  Cut as IrCut,
  Feed as IrFeed,
  Image as IrImage,
  Init as IrInit,
  PDF417 as IrPDF417,
  QRCode as IrQRCode,
  Rule as IrRule,
  Text as IrText,
  PrinterProfile,
  TextStyle,
} from '@imprentajs/escpos';
import type { ReactNode } from 'react';
import { host } from '../element.js';

export interface EscPosProps {
  profile?: PrinterProfile;
  children?: ReactNode;
}
export interface TextProps extends Omit<IrText, 'type' | 'text'> {
  children?: ReactNode;
}
export interface RowProps extends TextStyle {
  gap?: number;
  children?: ReactNode;
}
export interface ColumnProps extends Omit<IrColumn, 'text'> {
  children?: ReactNode;
}

/** A roll-paper job. Initialization is automatic and happens exactly once. */
export const EscPos = host<EscPosProps>('escpos');
export const Init = host<Omit<IrInit, 'type'>>('receipt-init');
/** One text block. Native character cells, rather than CSS or PDF points. */
export const Text = host<TextProps>('receipt-text');
/** Product columns, with continuation lines kept under their own column. */
export const Row = host<RowProps>('receipt-row');
export const Column = host<ColumnProps>('receipt-column');
export const Rule = host<Omit<IrRule, 'type'>>('receipt-rule');
export const QrCode = host<Omit<IrQRCode, 'type' | 'value'> & { children?: ReactNode }>(
  'receipt-qrcode',
);
export const BarCode = host<
  Omit<IrBarcode, 'type' | 'value' | 'barcodeType'> & {
    type?: IrBarcode['barcodeType'];
    children?: ReactNode;
  }
>('receipt-barcode');
export const PDF417 = host<Omit<IrPDF417, 'type' | 'value'> & { children?: ReactNode }>(
  'receipt-pdf417',
);
export const Image = host<Omit<IrImage, 'type'>>('receipt-image');
export const Feed = host<Omit<IrFeed, 'type'>>('receipt-feed');
export const Cut = host<Omit<IrCut, 'type'>>('receipt-cut');
export const Bytes = host<{ children?: ReactNode }>('receipt-bytes');
