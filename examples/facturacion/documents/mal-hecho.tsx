import { Box, Document, Image, Link, Table, Text } from '@imprentajs/react/pdf';

/**
 * A document that gets several things wrong on purpose.
 *
 * Every fault here is one the engine will happily print: it is not the
 * engine's job to refuse a four point heading. The checks panel is what
 * catches them, and this is what it looks like when it does.
 */
export default function MalHecho() {
  return (
    <Document margin={3}>
      <Text className="text-2xl font-bold mb-2">Lo que el panel encuentra</Text>

      <Text size={4} className="mb-2">
        Este párrafo está a cuatro puntos. Se imprime, y no se lee.
      </Text>

      <Text className="text-sm text-slate-300 mb-2">
        Este otro está en gris claro sobre blanco: en pantalla se adivina, en papel desaparece.
      </Text>

      <Table
        columns={[{ width: 60 }, { width: 'auto' }, { width: 80 }]}
        header={{ cells: [{ text: 'Uno' }, { text: 'Dos' }, { text: 'Tres' }] }}
        rows={[{ cells: [{ text: 'a' }, { text: 'b' }] }]}
        padding={4}
        spaceAfter={12}
      />

      {/* 240 pixels wide, printed at 400pt: 43 dpi. Fine on a screen. */}
      <Image src="logo" width={400} />

      {/* Wider than an A4 page leaves between its margins. */}
      <Box className="bg-slate-100" width={700} />

      <Link href="/condiciones">
        <Text className="text-sm">Un enlace que no lleva a ninguna parte desde un PDF</Text>
      </Link>
    </Document>
  );
}
