export interface Run {
  text: string;
  weight?: 'bold';
  italic?: true;
  color?: string;
}

export interface Edges {
  top?: number;
  right?: number;
  bottom?: number;
  left?: number;
}

/**
 * The IR, as the engine reads it.
 *
 * Named apart from the `<Document>` component on purpose: one is what an
 * author writes, the other is what comes out, and confusing them in an error
 * message would be its own small cruelty.
 */
export interface IrBand {
  height: number;
  children: IrNode[];
}

export interface IrDocument {
  page: { width: number; height: number; margin: Edges };
  header?: IrBand;
  footer?: IrBand;
  accumulators?: string[];
  children: IrNode[];
}

export type IrNode = { t: string } & Record<string, unknown>;
