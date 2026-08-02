export class SeenIdRing {
  private readonly ids: Uint32Array;
  private cursor = 0;

  constructor(size: number) {
    if (size <= 0 || (size & (size - 1)) !== 0) {
      throw new Error('SeenIdRing size must be a positive power of two');
    }
    this.ids = new Uint32Array(size);
  }

  has(idValue: number): boolean {
    const id = Math.trunc(idValue) >>> 0;
    if (id === 0) return true;

    for (let i = 0; i < this.ids.length; i += 1) {
      if (this.ids[i] === id) return true;
    }
    return false;
  }

  remember(idValue: number): void {
    const id = Math.trunc(idValue) >>> 0;
    if (id === 0) return;

    this.ids[this.cursor] = id;
    this.cursor = (this.cursor + 1) & (this.ids.length - 1);
  }

  rememberIfNew(idValue: number): boolean {
    if (this.has(idValue)) return false;
    this.remember(idValue);
    return true;
  }
}
