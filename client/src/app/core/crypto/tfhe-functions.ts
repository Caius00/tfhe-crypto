import init, { TfheConfigBuilder, TfheClientKey } from 'tfhe';

/**
 * Hier kommen alle TFHE-Funktionen rein.
 *
 * Vor der ersten Nutzung muss das WASM-Modul einmalig initialisiert werden:
 *   await init();
 *
 * Beispiel: Schlüssel generieren und in der Konsole ausgeben
 */
export async function printClientKey(): Promise<void> {
  await init();
  const config = TfheConfigBuilder.default().build();
  const clientKey = TfheClientKey.generate(config);
  console.log('ClientKey generiert:', clientKey);
}
