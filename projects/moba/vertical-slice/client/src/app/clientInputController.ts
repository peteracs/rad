export interface ClientInputControllerCallbacks {
  onResize(): void;
  onMoveCommand(clientX: number, clientY: number): void;
  onAimPreview(clientX: number, clientY: number): void;
  onAimCancel(): void;
  onCastCommand(clientX: number, clientY: number): void;
  onDebugToggle(enabled: boolean): void;
}

export class ClientInputController {
  private readonly resizeHandler = () => this.callbacks.onResize();
  private readonly contextMenuHandler = (event: MouseEvent) => event.preventDefault();
  private readonly pointerDownHandler = (event: PointerEvent) => this.onPointerDown(event);
  private readonly pointerMoveHandler = (event: PointerEvent) => this.onPointerMove(event);
  private readonly keyDownHandler = (event: KeyboardEvent) => this.onKeyDown(event);
  private readonly keyUpHandler = (event: KeyboardEvent) => this.onKeyUp(event);
  private readonly debugToggleHandler = (event: Event) => this.onDebugToggle(event);
  private lastPointerClientX = 0;
  private lastPointerClientY = 0;
  private hasPointer = false;
  private skillshotAiming = false;
  private bound = false;

  constructor(
    private readonly canvas: HTMLCanvasElement,
    private readonly callbacks: ClientInputControllerCallbacks,
  ) {}

  bind(): void {
    if (this.bound) return;
    this.bound = true;
    window.addEventListener('resize', this.resizeHandler, { passive: true });
    window.addEventListener('keydown', this.keyDownHandler);
    window.addEventListener('keyup', this.keyUpHandler);
    window.addEventListener('moba-rad-debug-toggle', this.debugToggleHandler);
    this.canvas.addEventListener('contextmenu', this.contextMenuHandler);
    this.canvas.addEventListener('pointerdown', this.pointerDownHandler);
    this.canvas.addEventListener('pointermove', this.pointerMoveHandler, { passive: true });
  }

  dispose(): void {
    if (!this.bound) return;
    this.bound = false;
    window.removeEventListener('resize', this.resizeHandler);
    window.removeEventListener('keydown', this.keyDownHandler);
    window.removeEventListener('keyup', this.keyUpHandler);
    window.removeEventListener('moba-rad-debug-toggle', this.debugToggleHandler);
    this.canvas.removeEventListener('contextmenu', this.contextMenuHandler);
    this.canvas.removeEventListener('pointerdown', this.pointerDownHandler);
    this.canvas.removeEventListener('pointermove', this.pointerMoveHandler);
  }

  private onPointerMove(event: PointerEvent): void {
    this.rememberPointer(event.clientX, event.clientY);
    if (this.skillshotAiming) {
      this.callbacks.onAimPreview(this.lastPointerClientX, this.lastPointerClientY);
    }
  }

  private onPointerDown(event: PointerEvent): void {
    this.rememberPointer(event.clientX, event.clientY);
    if (event.button !== 2) return;
    event.preventDefault();
    this.callbacks.onMoveCommand(this.lastPointerClientX, this.lastPointerClientY);
  }

  private onKeyDown(event: KeyboardEvent): void {
    if (event.repeat || event.code !== 'KeyQ') return;
    event.preventDefault();
    this.skillshotAiming = true;
    if (this.hasPointer) {
      this.callbacks.onAimPreview(this.lastPointerClientX, this.lastPointerClientY);
    } else {
      this.callbacks.onAimCancel();
    }
  }

  private onKeyUp(event: KeyboardEvent): void {
    if (event.code !== 'KeyQ') return;
    event.preventDefault();
    if (!this.skillshotAiming) return;
    this.skillshotAiming = false;
    this.callbacks.onAimCancel();
    if (this.hasPointer) {
      this.callbacks.onCastCommand(this.lastPointerClientX, this.lastPointerClientY);
    }
  }

  private onDebugToggle(event: Event): void {
    const customEvent = event as CustomEvent<{ enabled?: boolean }>;
    this.callbacks.onDebugToggle(customEvent.detail?.enabled === true);
  }

  private rememberPointer(clientX: number, clientY: number): void {
    this.lastPointerClientX = clientX;
    this.lastPointerClientY = clientY;
    this.hasPointer = true;
  }
}
