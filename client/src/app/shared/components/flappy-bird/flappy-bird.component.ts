import { Component, ElementRef, OnDestroy, AfterViewInit, ViewChild, HostListener, Output, EventEmitter } from '@angular/core';

interface Particle {
  x: number; y: number;
  vx: number; vy: number;
  alpha: number; size: number;
}

interface Pipe {
  x: number;
  gapY: number;
  scored: boolean;
}

type Phase =
  | 'TITLE_ENTER'
  | 'TITLE_SHOW'
  | 'BIRD_FLY_IN'
  | 'COLLISION'
  | 'BIRD_TUMBLE'
  | 'BIRD_RECOVER'
  | 'IDLE'
  | 'PLAYING'
  | 'GAME_OVER';

const SCROLL_SPEED   = 0.6;
const GREEN          = '#166534';
const TEXT_X_FRAC    = 0.5;
const PIPE_GAP       = 165;
const PIPE_WIDTH     = 52;
const PIPE_SPEED     = 160;
const PIPE_INTERVAL  = 2.0;
const BIRD_IDLE_X    = 90;
const GROUND_H       = 58;

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

  /** Emits the final score when a game ends. */
  @Output() gameOver = new EventEmitter<number>();

  private ctx!: CanvasRenderingContext2D;

  // Assets
  private bgImage  = loadImg('/games/flappy-bird/images/background.png');
  private birdUp   = loadImg('/games/flappy-bird/images/bird/yellowbird-upflap.png');
  private birdMid  = loadImg('/games/flappy-bird/images/bird/yellowbird-midflap.png');
  private birdDown = loadImg('/games/flappy-bird/images/bird/yellowbird-downflap.png');

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

  // Flap cycle for fly-in / idle
  private flapTimer = 0;
  private flapFrame = 0; // 0=up 1=mid 2=down 3=mid

  // Particles
  private particles: Particle[] = [];

  // Game state
  private pipes: Pipe[] = [];
  private pipeTimer = 0;
  private score = 0;
  private bestScore = 0;
  private gameOverTimer = 0;

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

  @HostListener('click')
  onClick(): void {
    if (this.phase === 'IDLE') {
      this.startGame();
    } else if (this.phase === 'PLAYING') {
      this.flap();
    } else if (this.phase === 'GAME_OVER' && this.gameOverTimer > 1.0) {
      this.startGame();
    }
  }

  private startGame(): void {
    const H = this.canvasRef.nativeElement.height;
    this.pipes = [];
    this.pipeTimer = 0;
    this.score = 0;
    this.bx = BIRD_IDLE_X;
    this.by = H * 0.45;
    this.bvx = 0;
    this.bvy = 0;
    this.brot = 0;
    this.setPhase('PLAYING');
    this.flap();
  }

  private flap(): void {
    this.bvy = -400;
    this.bvx = Math.min(this.bvx + 45, 90); // kleiner Rechts-Schub
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

    // Pipes & Ground nur im Spiel
    if (this.phase === 'PLAYING' || this.phase === 'GAME_OVER') {
      this.drawPipes(H);
      this.drawGround(W, H);
    }

    this.drawTitle(W, H);
    this.drawParticles();

    if (this.phase !== 'TITLE_ENTER' && this.phase !== 'TITLE_SHOW') {
      this.drawBird();
    }

    if (this.phase === 'PLAYING') {
      this.drawScore(W);
    }

    if (this.phase === 'GAME_OVER') {
      this.drawGround(W, H);
      this.drawScore(W);
      this.drawGameOver(W, H);
    }

    if (this.phase === 'IDLE') {
      this.drawStartPrompt(W, H);
    }

    this.animationId = requestAnimationFrame((t) => this.loop(t));
  }

  private setPhase(p: Phase): void { this.phase = p; this.pt = 0; }

  private update(W: number, H: number, dt: number): void {
    const midY = H * 0.45;

    // Flap cycle animation
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
        this.bvy  = Math.cos(this.pt * 9) * 90;
        this.brot = -0.15;
        if (this.textLeftEdge > 0 && this.bx + 24 >= this.textLeftEdge) {
          this.bx = this.textLeftEdge - 24;
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
        this.bvy += 380 * dt;
        this.bvy  = Math.min(this.bvy, 400);

        if (this.by >= midY + 100 && this.bvy > 0) {
          this.bvy = -390;
        }

        this.bvx += (BIRD_IDLE_X - this.bx) * 4 * dt;
        this.bvx *= Math.pow(0.05, dt);

        this.bx += this.bvx * dt;
        this.by += this.bvy * dt;

        const rot = Math.max(-0.5, Math.min(1.2, this.bvy / 300));
        this.brot += (rot - this.brot) * 10 * dt;

        if (this.phase === 'BIRD_RECOVER' && Math.abs(this.bx - BIRD_IDLE_X) < 25 && this.pt >= 0.8) {
          this.setPhase('IDLE');
        }
        break;
      }

      case 'PLAYING': {
        // Schwerkraft
        this.bvy += 600 * dt;
        this.bvy  = Math.min(this.bvy, 520);

        // Rechts-Schub klingt ab
        this.bvx *= Math.pow(0.08, dt);
        this.bx  += this.bvx * dt;
        this.by  += this.bvy * dt;

        // X innerhalb sinnvoller Grenzen halten
        if (this.bx < 50)         this.bx = 50;
        if (this.bx > W * 0.35)   this.bx = W * 0.35;

        // Rotation folgt Vertikalgeschwindigkeit
        const rot = Math.max(-0.5, Math.min(1.4, this.bvy / 350));
        this.brot += (rot - this.brot) * 12 * dt;

        // Pipes spawnen
        this.pipeTimer += dt;
        if (this.pipeTimer >= PIPE_INTERVAL) {
          this.pipeTimer = 0;
          const minGapY = 100 + PIPE_GAP / 2;
          const maxGapY = H - GROUND_H - 60 - PIPE_GAP / 2;
          const gapY = minGapY + Math.random() * (maxGapY - minGapY);
          this.pipes.push({ x: W + PIPE_WIDTH, gapY, scored: false });
        }

        // Pipes bewegen + Score zählen
        for (const pipe of this.pipes) {
          pipe.x -= PIPE_SPEED * dt;
          if (!pipe.scored && pipe.x + PIPE_WIDTH / 2 < this.bx) {
            pipe.scored = true;
            this.score++;
          }
        }
        this.pipes = this.pipes.filter(p => p.x > -PIPE_WIDTH - 10);

        // Kollisionsprüfung
        if (this.checkCollision(H)) {
          this.bestScore = Math.max(this.bestScore, this.score);
          this.gameOver.emit(this.score);
          this.setPhase('GAME_OVER');
          this.gameOverTimer = 0;
          this.bvx = -80;
          this.bvy = -200;
        }
        break;
      }

      case 'GAME_OVER': {
        this.gameOverTimer += dt;

        // Vogel fällt zu Boden
        this.bvy += 500 * dt;
        this.bvy  = Math.min(this.bvy, 600);
        this.bvx *= Math.pow(0.15, dt);
        this.bx  += this.bvx * dt;
        this.by  += this.bvy * dt;
        this.brot += 8 * dt;

        const groundY = H - GROUND_H - 18;
        if (this.by > groundY) {
          this.by   = groundY;
          this.bvy  = 0;
          this.bvx  = 0;
        }
        break;
      }
    }
  }

  private checkCollision(H: number): boolean {
    const birdR  = 13;
    const groundY = H - GROUND_H;

    if (this.by + birdR >= groundY) return true;
    if (this.by - birdR <= 0)       return true;

    for (const pipe of this.pipes) {
      const pLeft     = pipe.x - PIPE_WIDTH / 2;
      const pRight    = pipe.x + PIPE_WIDTH / 2 + 5; // leicht breiter für cap
      const gapTop    = pipe.gapY - PIPE_GAP / 2;
      const gapBottom = pipe.gapY + PIPE_GAP / 2;

      if (this.bx + birdR > pLeft && this.bx - birdR < pRight) {
        if (this.by - birdR < gapTop || this.by + birdR > gapBottom) {
          return true;
        }
      }
    }
    return false;
  }

  // ── Draw helpers ──────────────────────────────────────────────

  private drawPipes(H: number): void {
    const ctx     = this.ctx;
    const groundY = H - GROUND_H;
    const capH    = 26;
    const capW    = PIPE_WIDTH + 12;

    for (const pipe of this.pipes) {
      const gapTop    = pipe.gapY - PIPE_GAP / 2;
      const gapBottom = pipe.gapY + PIPE_GAP / 2;

      // --- obere Röhre ---
      // Rohr-Körper
      ctx.fillStyle = '#4ab34a';
      ctx.fillRect(pipe.x - PIPE_WIDTH / 2, 0, PIPE_WIDTH, gapTop - capH);
      // Highlight
      ctx.fillStyle = '#72d572';
      ctx.fillRect(pipe.x - PIPE_WIDTH / 2 + 4, 0, 10, gapTop - capH);
      // Cap
      ctx.fillStyle = '#4ab34a';
      ctx.fillRect(pipe.x - capW / 2, gapTop - capH, capW, capH);
      ctx.fillStyle = '#72d572';
      ctx.fillRect(pipe.x - capW / 2 + 4, gapTop - capH, 10, capH);
      // Outline
      ctx.strokeStyle = '#2e7d2e';
      ctx.lineWidth = 2;
      ctx.strokeRect(pipe.x - PIPE_WIDTH / 2, 0, PIPE_WIDTH, gapTop - capH);
      ctx.strokeRect(pipe.x - capW / 2, gapTop - capH, capW, capH);

      // --- untere Röhre ---
      const bodyTop = gapBottom + capH;
      const bodyH   = groundY - bodyTop;
      // Cap
      ctx.fillStyle = '#4ab34a';
      ctx.fillRect(pipe.x - capW / 2, gapBottom, capW, capH);
      ctx.fillStyle = '#72d572';
      ctx.fillRect(pipe.x - capW / 2 + 4, gapBottom, 10, capH);
      // Rohr-Körper
      ctx.fillStyle = '#4ab34a';
      ctx.fillRect(pipe.x - PIPE_WIDTH / 2, bodyTop, PIPE_WIDTH, bodyH);
      ctx.fillStyle = '#72d572';
      ctx.fillRect(pipe.x - PIPE_WIDTH / 2 + 4, bodyTop, 10, bodyH);
      // Outline
      ctx.strokeStyle = '#2e7d2e';
      ctx.lineWidth = 2;
      ctx.strokeRect(pipe.x - capW / 2, gapBottom, capW, capH);
      ctx.strokeRect(pipe.x - PIPE_WIDTH / 2, bodyTop, PIPE_WIDTH, bodyH);
    }
  }

  private drawGround(W: number, H: number): void {
    const ctx     = this.ctx;
    const groundY = H - GROUND_H;
    ctx.fillStyle = '#ded895';
    ctx.fillRect(0, groundY, W, GROUND_H);
    ctx.fillStyle = '#c8b84a';
    ctx.fillRect(0, groundY, W, 5);
    // Gras
    ctx.fillStyle = '#5a9e3a';
    ctx.fillRect(0, groundY - 6, W, 8);
  }

  private drawScore(W: number): void {
    const ctx  = this.ctx;
    const font = this.fontReady ? '"Press Start 2P"' : '"Courier New", monospace';
    ctx.save();
    ctx.font          = `28px ${font}`;
    ctx.textAlign     = 'center';
    ctx.textBaseline  = 'top';
    ctx.fillStyle     = '#ffffff';
    ctx.shadowColor   = '#000';
    ctx.shadowBlur    = 0;
    ctx.shadowOffsetX = 2;
    ctx.shadowOffsetY = 2;
    ctx.fillText(`${this.score}`, W / 2, 18);
    ctx.restore();
  }

  private drawGameOver(W: number, H: number): void {
    const ctx  = this.ctx;
    const font = this.fontReady ? '"Press Start 2P"' : '"Courier New", monospace';

    // Panel
    const panelW = W * 0.72;
    const panelH = H * 0.38;
    const panelX = (W - panelW) / 2;
    const panelY = H * 0.24;

    ctx.save();
    ctx.fillStyle = 'rgba(0,0,0,0.55)';
    this.roundRect(panelX, panelY, panelW, panelH, 12);
    ctx.fill();

    ctx.textAlign    = 'center';
    ctx.textBaseline = 'middle';
    ctx.shadowBlur   = 0;

    // GAME OVER
    ctx.font      = `${Math.min(W / 11, 30)}px ${font}`;
    ctx.fillStyle = '#ff4444';
    ctx.shadowOffsetX = 2; ctx.shadowOffsetY = 2;
    ctx.shadowColor = '#000';
    ctx.fillText('GAME OVER', W / 2, panelY + panelH * 0.22);

    // Scores
    ctx.font      = `${Math.min(W / 17, 18)}px ${font}`;
    ctx.fillStyle = '#ffffff';
    ctx.fillText(`Score: ${this.score}`,       W / 2, panelY + panelH * 0.48);
    ctx.fillText(`Best:  ${this.bestScore}`,   W / 2, panelY + panelH * 0.68);

    // Restart hint
    if (this.gameOverTimer > 1.0) {
      const pulse = 0.55 + 0.45 * Math.sin(this.gameOverTimer * 5);
      ctx.globalAlpha = pulse;
      ctx.font        = `${Math.min(W / 22, 13)}px ${font}`;
      ctx.fillStyle   = '#aaffaa';
      ctx.shadowOffsetX = 1; ctx.shadowOffsetY = 1;
      ctx.fillText('Click to Restart', W / 2, panelY + panelH * 0.88);
    }

    ctx.restore();
  }

  private drawStartPrompt(W: number, H: number): void {
    const ctx   = this.ctx;
    const font  = this.fontReady ? '"Press Start 2P"' : '"Courier New", monospace';
    const pulse = 0.55 + 0.45 * Math.sin(this.pt * 3.5);

    ctx.save();
    ctx.globalAlpha   = pulse;
    ctx.font          = `${Math.min(W / 22, 15)}px ${font}`;
    ctx.textAlign     = 'center';
    ctx.textBaseline  = 'middle';
    ctx.fillStyle     = '#ffffff';
    ctx.shadowColor   = '#000';
    ctx.shadowOffsetX = 2; ctx.shadowOffsetY = 2;
    ctx.shadowBlur    = 0;
    ctx.fillText('Click to Play!', W / 2, H * 0.73);
    ctx.restore();
  }

  private roundRect(x: number, y: number, w: number, h: number, r: number): void {
    const ctx = this.ctx;
    ctx.beginPath();
    ctx.moveTo(x + r, y);
    ctx.lineTo(x + w - r, y);
    ctx.quadraticCurveTo(x + w, y, x + w, y + r);
    ctx.lineTo(x + w, y + h - r);
    ctx.quadraticCurveTo(x + w, y + h, x + w - r, y + h);
    ctx.lineTo(x + r, y + h);
    ctx.quadraticCurveTo(x, y + h, x, y + h - r);
    ctx.lineTo(x, y + r);
    ctx.quadraticCurveTo(x, y, x + r, y);
    ctx.closePath();
  }

  /** Pick the right bird image based on vertical velocity */
  private getBirdFrame(): HTMLImageElement {
    if (this.phase === 'BIRD_TUMBLE' || this.phase === 'GAME_OVER') return this.birdDown;

    if (this.phase === 'BIRD_FLY_IN' || this.phase === 'IDLE' ||
        this.phase === 'BIRD_RECOVER' || this.phase === 'PLAYING') {
      if (this.bvy < -40) return this.birdUp;
      if (this.bvy >  40) return this.birdDown;
      return this.birdMid;
    }

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

    // Measure text left edge for collision
    const tw = ctx.measureText('Clappy-Bird').width;
    this.textLeftEdge = x - tw / 2;

    // Drop shadow
    ctx.fillStyle = '#003300';
    ctx.shadowBlur = 0; ctx.shadowOffsetX = 3; ctx.shadowOffsetY = 3;
    ctx.fillText('Clappy-Bird', x, H * 0.45);

    // Black outline
    ctx.strokeStyle = '#000000';
    ctx.lineWidth = Math.max(3, size * 0.12);
    ctx.lineJoin = 'round';
    ctx.strokeText('Clappy-Bird', x, H * 0.45);

    // Fill
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
