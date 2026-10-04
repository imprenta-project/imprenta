import type { Receipt, ReceiptNode } from '@imprentajs/escpos';
import { type Instance, isText, type Node } from '../host.js';

const STYLE = [
  'justification',
  'font',
  'bold',
  'underline',
  'inverse',
  'fontWidth',
  'fontHeight',
  'lineSpacing',
  'doubleStrike',
];
const PROPS: Record<string, string[]> = {
  'receipt-init': ['width', 'marginLeft', 'dpi'],
  'receipt-text': [...STYLE, 'lf', 'prefix', 'suffix', 'format'],
  'receipt-row': [...STYLE, 'gap'],
  'receipt-column': ['width', 'justification', 'overflow'],
  'receipt-rule': [...STYLE, 'character'],
  'receipt-qrcode': ['justification', 'model', 'size', 'errorCorrectionLevel'],
  'receipt-barcode': ['justification', 'type', 'width', 'height', 'hriPosition', 'hriFont'],
  'receipt-pdf417': [
    'justification',
    'width',
    'height',
    'numberOfColumns',
    'numberOfRows',
    'errorCorrectionLevel',
    'option',
  ],
  'receipt-image': ['src', 'width', 'height', 'justification', 'filter', 'threshold'],
  'receipt-feed': ['lines'],
  'receipt-cut': ['mode'],
  'receipt-bytes': [],
};

function props(node: Instance, allowed: string[]): Record<string, unknown> {
  const result: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(node.props)) {
    if (key === 'children' || value === undefined) continue;
    if (!allowed.includes(key)) throw new Error(`<${node.type}> does not support ${key}`);
    result[key] = value;
  }
  return result;
}

function text(nodes: Node[]): string {
  return nodes
    .map((node) => {
      if (!isText(node)) throw new Error(`receipt text holds characters, not <${node.type}>`);
      return node.text;
    })
    .join('');
}

export function toIr(root: Instance): Receipt {
  const settings = props(root, ['profile']);
  const children: ReceiptNode[] = [];
  for (const node of root.children) {
    if (isText(node)) {
      if (node.text.trim() === '') continue;
      throw new Error('put receipt content inside a <Text> or a <Row>');
    }
    const allowed = PROPS[node.type];
    if (!allowed || node.type === 'receipt-column')
      throw new Error(`an <EscPos> cannot contain <${node.type}>`);
    const values = props(node, allowed);
    const type = node.type.slice('receipt-'.length);
    if (type === 'text') {
      children.push({ type: 'text', ...values, text: text(node.children) });
    } else if (type === 'row') {
      const columns = node.children
        .filter((child) => !isText(child) || child.text.trim() !== '')
        .map((child) => {
          if (isText(child) || child.type !== 'receipt-column')
            throw new Error('a <Row> contains <Column> elements only');
          return { ...props(child, PROPS['receipt-column']), text: text(child.children) };
        });
      children.push({ type: 'row', ...values, columns });
    } else if (type === 'qrcode' || type === 'barcode' || type === 'pdf417') {
      if (type === 'barcode' && values.type !== undefined) {
        values.barcodeType = values.type;
        delete values.type;
      }
      children.push({ type, ...values, value: text(node.children) } as ReceiptNode);
    } else if (type === 'bytes') {
      children.push({ type, hex: text(node.children) });
    } else {
      if (node.children.length) throw new Error(`<${node.type}> does not take children`);
      children.push({ type, ...values } as ReceiptNode);
    }
  }
  return { ...settings, children };
}
