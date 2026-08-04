import { B, Box, Document, Text } from '@imprentajs/react/pdf';

interface Props {
  month: string;
  entries: { label: string; note: string }[];
}

export default function Informe({ month, entries }: Props) {
  return (
    <Document className="p-10">
      <Text className="text-2xl font-bold mb-1">Informe mensual</Text>
      <Text className="text-sm text-slate-500 mb-6">{month}</Text>

      {entries.map((entry) => (
        <Box key={entry.label} className="border-b border-slate-200 pb-3 mb-3">
          <Text className="text-base font-bold">{entry.label}</Text>
          <Text className="text-sm text-slate-700">{entry.note}</Text>
        </Box>
      ))}

      <Text className="text-xs text-slate-500">
        Un documento sin ninguna tabla, para comprobar que <B>no hace falta una</B>.
      </Text>
    </Document>
  );
}

Informe.PreviewProps = {
  month: 'Julio de 2026',
  entries: [
    { label: 'Facturación', note: 'Un catorce por ciento por encima del mes anterior.' },
    { label: 'Impagos', note: 'Dos expedientes abiertos, ambos con acuerdo de pago.' },
    { label: 'Soporte', note: 'Tiempo medio de respuesta de dos horas y once minutos.' },
  ],
} satisfies Props;
