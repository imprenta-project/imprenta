import { Cell, Image, Row, Sheet, Workbook } from '@imprentajs/react/xlsx';

/**
 * A workbook that names an image the project has not got.
 *
 * Its own document rather than another fault in `mal-hecha.tsx`, and the reason
 * is the difference between the two kinds of fault. Everything in `mal-hecha`
 * produces a file: it opens, somebody uses it, and the panel is the only thing
 * that will ever tell them it is wrong. This one produces nothing — the engine
 * refuses to write a workbook with a hole where the logo was — so putting it
 * beside the others would take the whole file down and hide the five faults
 * that were the point of it.
 *
 * What is being shown here is the wording. The engine's own message names the
 * image and nothing else, so `missing-image` runs before the write and says
 * which sheet, in a workbook that may have twenty.
 */
export default function SinImagen() {
  return (
    <Workbook>
      <Sheet name="Ventas">
        <Row>
          {/* `imprenta.config.ts` configures `logo`. Nothing is called
              `membrete`, and a typo in a name is the whole failure mode: the
              author sees a build that stopped and no reason for it. */}
          <Cell>
            <Image src="membrete" width={90} />
            Concepto
          </Cell>
        </Row>
      </Sheet>
    </Workbook>
  );
}
