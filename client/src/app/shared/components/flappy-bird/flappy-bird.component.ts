import { Component, ElementRef, OnDestroy, AfterViewInit, ViewChild } from '@angular/core';

interface Particle {
  x: number; y: number;
  vx: number; vy: number;
  alpha: number; size: number;
}

type Phase =
  | 'TITLE_ENTER'
  | 'TITLE_SHOW'
  | 'BIRD_FLY_IN'
  | 'COLLISION'
  | 'BIRD_TUMBLE'
  | 'BIRD_RECOVER'
  | 'IDLE';

const SCROLL_SPEED = 0.6;
const GREEN        = '#166534';
const TEXT_X_FRAC  = 0.5;

function easeOutBack(t: number): number {
  const c1 = 1.70158, c3 = c1 + 1;
  return 1 + c3 * Math.pow(t - 1, 3) + c1 * Math.pow(t - 1, 2);
}

function loadImg(src: string): HTMLImageElement {
  const img = new Image();
  img.src = src;
  return img;
}

@Component({
  selector: 'app-flappy-bird',
  templateUrl: './flappy-bird.component.html',
  styleUrl: './flappy-bird.component.css',
})
export class FlappyBirdComponent implements AfterViewInit, OnDestroy {
  @ViewChild('canvas') canvasRef!: ElementRef<HTMLCanvasElement>;
  @ViewChild('gameWindow') windowRef!: ElementRef<HTMLDivElement>;

  private ctx!: CanvasRenderingContext2D;

  // Assets
  private bgImage   = loadImg('/games/flappy-bird/images/background.png');
  private birdUp    = loadImg('/games/flappy-bird/images/bird/yellowbird-upflap.png');
  private birdMid   = loadImg('/games/flappy-bird/images/bird/yellowbird-midflap.png');
  private birdDown  = loadImg('/games/flappy-bird/images/bird/yellowbird-downflap.png');

  private bgX = 0;
  private animationId = 0;
  private resizeObserver!: ResizeObserver;
  private lastTs = 0;
  private fontReady = false;

  // measured left edge of the title text (updated each draw)
  private textLeftEdge = 0;

  // Phase
  private phase: Phase = 'TITLE_ENTER';
  private pt = 0;

  // Bird
  private bx = -80;
  private by = 0;
  private bvx = 0;
  private bvy = 0;
  private brot = 0;

  // Flap cycle for fly-in / idle (independent of vy-based skin)
  private flapTimer = 0;
  private flapFrame = 0; // 0=up 1=mid 2=down 3=mid


  // Particles
  private particles: Particle[] = [];

  ngAfterViewInit(): void {
    this.ctx = this.canvasRef.nativeElement.getContext('2d')!;

    this.resizeObserver = new ResizeObserver(() => this.resizeCanvas());
    this.resizeObserver.observe(this.windowRef.nativeElement);
    this.resizeCanvas();

    document.fonts.load('16px "Press Start 2P"').then(() => {
      this.fontReady = true;
    });

    this.animationId = requestAnimationFrame((t) => { this.lastTs = t; this.loop(t); });
  }

  private resizeCanvas(): void {
    const el = this.windowRef.nativeElement;
    const c  = this.canvasRef.nativeElement;
    c.width  = el.clientWidth;
    c.height = el.clientHeight;
  }

  private loop(ts: number): void {
    const dt = Math.min((ts - this.lastTs) / 1000, 0.05);
    this.lastTs = ts;
    this.pt += dt;

    const { width: W, height: H } = this.canvasRef.nativeElement;

    this.bgX -= SCROLL_SPEED;
    if (this.bgX <= -W) this.bgX = 0;

    this.drawBackground(W, H);
    this.update(W, H, dt);
    this.drawTitle(W, H);
    this.drawParticles();

    if (this.phase !== 'TITLE_ENTER' && this.phase !== 'TITLE_SHOW') {
      this.drawBird();
    }

    this.animationId = requestAnimationFrame((t) => this.loop(t));
  }

  private setPhase(p: Phase): void { this.phase = p; this.pt = 0; }

  private update(W: number, H: number, dt: number): void {
    const midY = H * 0.45;

    // Flap cycle animation (up→mid→down→mid→up…)
    this.flapTimer += dt;
    if (this.flapTimer > 0.11) {
      this.flapFrame = (this.flapFrame + 1) % 4;
      this.flapTimer = 0;
    }

    switch (this.phase) {

      case 'TITLE_ENTER':
        if (this.pt >= 1.8) this.setPhase('TITLE_SHOW');
        break;

      case 'TITLE_SHOW':
        if (this.pt >= 1.0) {
          this.setPhase('BIRD_FLY_IN');
          this.bx  = -80;
          this.by  = midY;
          this.bvx = 440;
          this.bvy = 0;
        }
        break;

      case 'BIRD_FLY_IN':
        this.bx  += this.bvx * dt;
        this.by   = midY + Math.sin(this.pt * 9) * 10;
        this.bvy  = Math.cos(this.pt * 9) * 90; // track vy for skin selection
        this.brot = -0.15;
        // collide at measured left edge of text
        if (this.textLeftEdge > 0 && this.bx + 24 >= this.textLeftEdge) {
          this.bx = this.textLeftEdge - 24; // snap to edge, no overshoot
          this.setPhase('COLLISION');
          this.spawnParticles(W, H);
          this.bvx = -380;
          this.bvy = -220;
        }
        break;

      case 'COLLISION':
        if (this.pt >= 0.06) this.setPhase('BIRD_TUMBLE');
        break;

      case 'BIRD_TUMBLE':
        this.bx  += this.bvx * dt;
        this.by  += this.bvy * dt;
        this.bvy += 420 * dt;
        this.bvx += (0 - this.bvx) * 2 * dt;
        this.brot += 9 * dt;
        for (const p of this.particles) {
          p.x += p.vx * dt; p.y += p.vy * dt;
          p.vy += 280 * dt; p.alpha -= dt * 1.0;
        }
        this.particles = this.particles.filter((p) => p.alpha > 0);
        if (this.pt >= 1.6) this.setPhase('BIRD_RECOVER');
        break;

      case 'BIRD_RECOVER':
      case 'IDLE': {
        // Schwerkraft
        this.bvy += 380 * dt;
        this.bvy  = Math.min(this.bvy, 400);

        // Sprung wenn Vogel halbe Sprunghöhe unter der Mitte ist
        // Sprunghöhe ≈ 200px → Trigger bei midY+100, Scheitelpunkt bei midY-100
        if (this.by >= midY + 100 && this.bvy > 0) {
          this.bvy = -390;
        }

        // X sanft zur Startposition
        this.bvx += (90 - this.bx) * 4 * dt;
        this.bvx *= Math.pow(0.05, dt);

        this.bx += this.bvx * dt;
        this.by += this.bvy * dt;

        // Rotation folgt der Vertikalgeschwindigkeit
        const rot = Math.max(-0.5, Math.min(1.2, this.bvy / 300));
        this.brot += (rot - this.brot) * 10 * dt;

        if (this.phase === 'BIRD_RECOVER' && Math.abs(this.bx - 90) < 25 && this.pt >= 0.8) {
          this.setPhase('IDLE');
        }
        break;
      }
    }
  }

  /** Pick the right bird image based on vertical velocity */
  private getBirdFrame(): HTMLImageElement {
    // During tumble: use downflap (falling/spinning)
    if (this.phase === 'BIRD_TUMBLE') return this.birdDown;

    // During fly-in / idle: cycle through animation frames
    if (this.phase === 'BIRD_FLY_IN' || this.phase === 'IDLE' || this.phase === 'BIRD_RECOVER') {
      if (this.bvy < -40)  return this.birdUp;
      if (this.bvy > 40)   return this.birdDown;
      return this.birdMid;
    }

    // Default flap cycle
    const frames = [this.birdUp, this.birdMid, this.birdDown, this.birdMid];
    return frames[this.flapFrame];
  }

  private spawnParticles(W: number, H: number): void {
    const OW = 720, OH = 110;
    const off = document.createElement('canvas');
    off.width = OW; off.height = OH;
    const ctx = off.getContext('2d')!;
    const font = this.fontReady ? '"Press Start 2P"' : '"Courier New", monospace';
    ctx.font = `36px ${font}`;
    ctx.fillStyle = GREEN;
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText('Clappy-Bird', OW / 2, OH / 2);

    const data = ctx.getImageData(0, 0, OW, OH).data;
    const S  = 5;
    const ox = W * TEXT_X_FRAC - OW / 2;
    const oy = H * 0.45 - OH / 2;

    for (let py = 0; py < OH; py += S) {
      for (let px = 0; px < OW; px += S) {
        if (data[(py * OW + px) * 4 + 3] > 100) {
          this.particles.push({
            x: ox + px, y: oy + py,
            vx: (px - OW / 2) * 1.0 + (Math.random() - 0.5) * 140,
            vy: (Math.random() - 0.5) * 180 - 80,
            alpha: 1, size: S,
          });
        }
      }
    }
  }

  private drawBackground(W: number, H: number): void {
    if (this.bgImage.complete && this.bgImage.naturalWidth > 0) {
      this.ctx.drawImage(this.bgImage, this.bgX, 0, W, H);
      this.ctx.drawImage(this.bgImage, this.bgX + W, 0, W, H);
    } else {
      this.ctx.clearRect(0, 0, W, H);
    }
  }

  private drawTitle(W: number, H: number): void {
    if (
      this.phase !== 'TITLE_ENTER' &&
      this.phase !== 'TITLE_SHOW' &&
      this.phase !== 'BIRD_FLY_IN'
    ) return;

    const ctx  = this.ctx;
    const font = this.fontReady ? '"Press Start 2P"' : '"Courier New", monospace';
    const size = Math.max(30, Math.min(W / 10, 74));

    let x = W / 2, alpha = 1;

    if (this.phase === 'TITLE_ENTER') {
      const prog = Math.min(this.pt / 1.8, 1);
      const ease = easeOutBack(prog);
      x     = W / 2 + (1 - ease) * (W / 2 + 300);
      alpha = Math.min(prog * 4, 1);
    }

    ctx.save();
    ctx.globalAlpha  = alpha;
    ctx.font         = `${size}px ${font}`;
    ctx.textAlign    = 'center';
    ctx.textBaseline = 'middle';
    ctx.imageSmoothingEnabled = false;

    // measure text left edge for collision
    const tw = ctx.measureText('Clappy-Bird').width;
    this.textLeftEdge = x - tw / 2;

    // drop shadow
    ctx.fillStyle = '#003300';
    ctx.shadowBlur = 0; ctx.shadowOffsetX = 3; ctx.shadowOffsetY = 3;
    ctx.fillText('Clappy-Bird', x, H * 0.45);

    // black outline for stronger contrast
    ctx.strokeStyle = '#000000';
    ctx.lineWidth = Math.max(3, size * 0.12);
    ctx.lineJoin = 'round';
    ctx.strokeText('Clappy-Bird', x, H * 0.45);

    // fill without glow
    ctx.shadowOffsetX = 0; ctx.shadowOffsetY = 0;
    ctx.shadowColor   = 'transparent'; ctx.shadowBlur = 0;
    ctx.fillStyle     = GREEN;
    ctx.fillText('Clappy-Bird', x, H * 0.45);

    ctx.restore();
  }

  private drawParticles(): void {
    const ctx = this.ctx;
    ctx.save();
    ctx.shadowColor = GREEN; ctx.shadowBlur = 6;
    ctx.fillStyle   = GREEN;
    for (const p of this.particles) {
      ctx.globalAlpha = Math.max(0, p.alpha);
      ctx.fillRect(p.x, p.y, p.size, p.size);
    }
    ctx.restore();
  }

  private drawBird(): void {
    const img = this.getBirdFrame();
    if (!img.complete || !img.naturalWidth) return;

    const W = 48, H = 36;
    const ctx = this.ctx;
    ctx.save();
    ctx.translate(this.bx, this.by);
    ctx.rotate(this.brot);
    ctx.imageSmoothingEnabled = false;
    ctx.drawImage(img, -W / 2, -H / 2, W, H);
    ctx.restore();
  }

  ngOnDestroy(): void {
    cancelAnimationFrame(this.animationId);
    this.resizeObserver?.disconnect();
  }
}
