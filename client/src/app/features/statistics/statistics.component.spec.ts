import { describe, it, expect } from 'vitest';
import { selectOptimalBitWidth } from './statistics.component';

describe('selectOptimalBitWidth', () => {
  // --- Int8-Bereich [-128, 127] ---

  it('gibt 8 zurück für eine Liste vollständig im Int8-Bereich', () => {
    expect(selectOptimalBitWidth([-5, 3, 7, 5, 33])).toBe(8);
  });

  it('gibt 8 zurück für Grenzwerte -128 und 127', () => {
    expect(selectOptimalBitWidth([-128, 127])).toBe(8);
  });

  it('gibt 8 zurück für eine einzelne Null', () => {
    expect(selectOptimalBitWidth([0])).toBe(8);
  });

  // --- Überschreitung der Int8-Grenze → Int16 ---

  it('gibt 16 zurück wenn ein Wert genau 128 ist', () => {
    expect(selectOptimalBitWidth([0, 128])).toBe(16);
  });

  it('gibt 16 zurück wenn ein Wert genau -129 ist', () => {
    expect(selectOptimalBitWidth([-129, 0])).toBe(16);
  });

  it('gibt 16 zurück für eine Liste im Int16-Bereich', () => {
    expect(selectOptimalBitWidth([-500, 300, 1000])).toBe(16);
  });

  it('gibt 16 zurück für Grenzwerte -32768 und 32767', () => {
    expect(selectOptimalBitWidth([-32_768, 32_767])).toBe(16);
  });

  // --- Überschreitung der Int16-Grenze → Int32 ---

  it('gibt 32 zurück wenn ein Wert genau 32768 ist', () => {
    expect(selectOptimalBitWidth([0, 32_768])).toBe(32);
  });

  it('gibt 32 zurück wenn ein Wert genau -32769 ist', () => {
    expect(selectOptimalBitWidth([-32_769, 0])).toBe(32);
  });

  it('gibt 32 zurück für große Werte', () => {
    expect(selectOptimalBitWidth([-50_000, 200_000])).toBe(32);
  });

  it('gibt 32 zurück für i32-Grenzwerte', () => {
    expect(selectOptimalBitWidth([-2_147_483_648, 2_147_483_647])).toBe(32);
  });

  // --- Gemischte Listen: das Maximum entscheidet ---

  it('gibt 16 zurück wenn Int8-Werte und ein 16-Wert gemischt sind', () => {
    expect(selectOptimalBitWidth([-5, 3, 1000])).toBe(16);
  });

  it('gibt 32 zurück wenn Int16-Werte und ein 32-Wert gemischt sind', () => {
    expect(selectOptimalBitWidth([100, 50_000])).toBe(32);
  });
});
