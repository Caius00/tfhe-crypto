import { Component, ElementRef, OnDestroy, AfterViewInit, ViewChild, HostListener, Output, EventEmitter } from '@angular/core';

interface ExplosionParticle {
  x: number; y: number;
  velocityX: number; velocityY: number;
  alpha: number; size: number;
}

interface Pipe {
  x: number;
  gapCenterY: number;
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

const BACKGROUND_SCROLL_SPEED          = 0.6;
const TITLE_GREEN_COLOR                = '#166534';
const TITLE_HORIZONTAL_CENTER_FRACTION = 0.5;
const PIPE_VERTICAL_GAP                = 165;
const PIPE_WIDTH                       = 52;
const PIPE_SCROLL_SPEED                = 160;
const PIPE_SPAWN_INTERVAL_SECONDS      = 2.0;
const BIRD_IDLE_X_POSITION             = 90;
const GROUND_HEIGHT                    = 58;

function easeOutBack(t: number): number {
  const c1 = 1.70158, c3 = c1 + 1;
  return 1 + c3 * Math.pow(t - 1, 3) + c1 * Math.pow(t - 1, 2);
}

function loadImage(src: string): HTMLImageElement {
  const image = new Image();
  image.src = src;
  return image;
}

@Component({
  selector: 'app-flappy-bird',
  templateUrl: './flappy-bird.component.html',
  styleUrl: './flappy-bird.component.css',
})
export class FlappyBirdComponent implements AfterViewInit, OnDestroy {
  @ViewChild('canvas') canvasRef!: ElementRef<HTMLCanvasElement>;
  @ViewChild('gameWindow') gameWindowRef!: ElementRef<HTMLDivElement>;

  /** Emits the final score when a game ends. */
  @Output() gameOver = new EventEmitter<number>();

  private canvasContext!: CanvasRenderingContext2D;

  // Assets
  private backgroundImage    = loadImage('/games/flappy-bird/images/background.png');
  private birdSpriteWingsUp  = loadImage('/games/flappy-bird/images/bird/yellowbird-upflap.png');
  private birdSpriteWingsMid = loadImage('/games/flappy-bird/images/bird/yellowbird-midflap.png');
  private birdSpriteWingsDown = loadImage('/games/flappy-bird/images/bird/yellowbird-downflap.png');

  private backgroundScrollX = 0;
  private animationFrameId = 0;
  private resizeObserver!: ResizeObserver;
  private lastFrameTimestamp = 0;
  private isFontLoaded = false;

  // measured left edge of the title text (updated each draw)
  private titleTextLeftEdge = 0;

  // Phase
  private currentPhase: Phase = 'TITLE_ENTER';
  private phaseElapsedSeconds = 0;

  // Bird
  private birdX = -80;
  private birdY = 0;
  private birdVelocityX = 0;
  private birdVelocityY = 0;
  private birdRotation = 0;

  // Flap cycle for fly-in / idle
  private flapAnimationTimer = 0;
  private flapAnimationFrame = 0; // 0=up 1=mid 2=down 3=mid

  // Particles
  private titleExplosionParticles: ExplosionParticle[] = [];

  // Game state
  private pipes: Pipe[] = [];
  private pipeSpawnTimer = 0;
  private score = 0;
  private bestScore = 0;
  private gameOverElapsedSeconds = 0;

  ngAfterViewInit(): void {
    this.canvasContext = this.canvasRef.nativeElement.getContext('2d')!;

    this.resizeObserver = new ResizeObserver(() => this.resizeCanvas());
    this.resizeObserver.observe(this.gameWindowRef.nativeElement);
    this.resizeCanvas();

    document.fonts.load('16px "Press Start 2P"').then(() => {
      this.isFontLoaded = true;
    });

    this.animationFrameId = requestAnimationFrame((timestamp) => {
      this.lastFrameTimestamp = timestamp;
      this.loop(timestamp);
    });
  }

  @HostListener('click')
  onClick(): void {
    this.handleJumpInput();
  }

  @HostListener('window:keydown', ['$event'])
  onKeyDown(event: KeyboardEvent): void {
    if (event.code !== 'Space' && event.key !== ' ') return;
    event.preventDefault();
    this.handleJumpInput();
  }

  private handleJumpInput(): void {
    if (this.currentPhase === 'IDLE') {
      this.startGame();
    } else if (this.currentPhase === 'PLAYING') {
      this.flap();
    } else if (this.currentPhase === 'GAME_OVER' && this.gameOverElapsedSeconds > 1.0) {
      this.startGame();
    }
  }

  private startGame(): void {
    const canvasHeight = this.canvasRef.nativeElement.height;
    this.pipes = [];
    this.pipeSpawnTimer = 0;
    this.score = 0;
    this.birdX = BIRD_IDLE_X_POSITION;
    this.birdY = canvasHeight * 0.45;
    this.birdVelocityX = 0;
    this.birdVelocityY = 0;
    this.birdRotation = 0;
    this.setPhase('PLAYING');
    this.flap();
  }

  private flap(): void {
    this.birdVelocityY = -400;
    this.birdVelocityX = Math.min(this.birdVelocityX + 45, 90); // kleiner Rechts-Schub
  }

  private resizeCanvas(): void {
    const containerElement = this.gameWindowRef.nativeElement;
    const canvasElement    = this.canvasRef.nativeElement;
    canvasElement.width  = containerElement.clientWidth;
    canvasElement.height = containerElement.clientHeight;
  }

  private loop(timestamp: number): void {
    const deltaSeconds = Math.min((timestamp - this.lastFrameTimestamp) / 1000, 0.05);
    this.lastFrameTimestamp = timestamp;
    this.phaseElapsedSeconds += deltaSeconds;

    const { width: canvasWidth, height: canvasHeight } = this.canvasRef.nativeElement;

    this.backgroundScrollX -= BACKGROUND_SCROLL_SPEED;
    if (this.backgroundScrollX <= -canvasWidth) this.backgroundScrollX = 0;

    this.drawBackground(canvasWidth, canvasHeight);
    this.update(canvasWidth, canvasHeight, deltaSeconds);

    // Pipes & Ground nur im Spiel
    if (this.currentPhase === 'PLAYING' || this.currentPhase === 'GAME_OVER') {
      this.drawPipes(canvasHeight);
      this.drawGround(canvasWidth, canvasHeight);
    }

    this.drawTitle(canvasWidth, canvasHeight);
    this.drawParticles();

    if (this.currentPhase !== 'TITLE_ENTER' && this.currentPhase !== 'TITLE_SHOW') {
      this.drawBird();
    }

    if (this.currentPhase === 'PLAYING') {
      this.drawScore(canvasWidth);
    }

    if (this.currentPhase === 'GAME_OVER') {
      this.drawGround(canvasWidth, canvasHeight);
      this.drawScore(canvasWidth);
      this.drawGameOver(canvasWidth, canvasHeight);
    }

    if (this.currentPhase === 'IDLE') {
      this.drawStartPrompt(canvasWidth, canvasHeight);
    }

    this.animationFrameId = requestAnimationFrame((nextTimestamp) => this.loop(nextTimestamp));
  }

  private setPhase(nextPhase: Phase): void {
    this.currentPhase = nextPhase;
    this.phaseElapsedSeconds = 0;
  }

  private update(canvasWidth: number, canvasHeight: number, deltaSeconds: number): void {
    const verticalCenter = canvasHeight * 0.45;

    // Flap cycle animation
    this.flapAnimationTimer += deltaSeconds;
    if (this.flapAnimationTimer > 0.11) {
      this.flapAnimationFrame = (this.flapAnimationFrame + 1) % 4;
      this.flapAnimationTimer = 0;
    }

    switch (this.currentPhase) {

      case 'TITLE_ENTER':
        if (this.phaseElapsedSeconds >= 1.8) this.setPhase('TITLE_SHOW');
        break;

      case 'TITLE_SHOW':
        if (this.phaseElapsedSeconds >= 1.0) {
          this.setPhase('BIRD_FLY_IN');
          this.birdX = -80;
          this.birdY = verticalCenter;
          this.birdVelocityX = 440;
          this.birdVelocityY = 0;
        }
        break;

      case 'BIRD_FLY_IN':
        this.birdX += this.birdVelocityX * deltaSeconds;
        this.birdY = verticalCenter + Math.sin(this.phaseElapsedSeconds * 9) * 10;
        this.birdVelocityY = Math.cos(this.phaseElapsedSeconds * 9) * 90;
        this.birdRotation = -0.15;
        if (this.titleTextLeftEdge > 0 && this.birdX + 24 >= this.titleTextLeftEdge) {
          this.birdX = this.titleTextLeftEdge - 24;
          this.setPhase('COLLISION');
          this.spawnTitleExplosionParticles(canvasWidth, canvasHeight);
          this.birdVelocityX = -380;
          this.birdVelocityY = -220;
        }
        break;

      case 'COLLISION':
        if (this.phaseElapsedSeconds >= 0.06) this.setPhase('BIRD_TUMBLE');
        break;

      case 'BIRD_TUMBLE':
        this.birdX += this.birdVelocityX * deltaSeconds;
        this.birdY += this.birdVelocityY * deltaSeconds;
        this.birdVelocityY += 420 * deltaSeconds;
        this.birdVelocityX += (0 - this.birdVelocityX) * 2 * deltaSeconds;
        this.birdRotation += 9 * deltaSeconds;
        for (const particle of this.titleExplosionParticles) {
          particle.x += particle.velocityX * deltaSeconds;
          particle.y += particle.velocityY * deltaSeconds;
          particle.velocityY += 280 * deltaSeconds;
          particle.alpha -= deltaSeconds * 1.0;
        }
        this.titleExplosionParticles = this.titleExplosionParticles.filter((particle) => particle.alpha > 0);
        if (this.phaseElapsedSeconds >= 1.6) this.setPhase('BIRD_RECOVER');
        break;

      case 'BIRD_RECOVER':
      case 'IDLE': {
        this.birdVelocityY += 380 * deltaSeconds;
        this.birdVelocityY = Math.min(this.birdVelocityY, 400);

        if (this.birdY >= verticalCenter + 100 && this.birdVelocityY > 0) {
          this.birdVelocityY = -390;
        }

        this.birdVelocityX += (BIRD_IDLE_X_POSITION - this.birdX) * 4 * deltaSeconds;
        this.birdVelocityX *= Math.pow(0.05, deltaSeconds);

        this.birdX += this.birdVelocityX * deltaSeconds;
        this.birdY += this.birdVelocityY * deltaSeconds;

        const targetRotation = Math.max(-0.5, Math.min(1.2, this.birdVelocityY / 300));
        this.birdRotation += (targetRotation - this.birdRotation) * 10 * deltaSeconds;

        if (this.currentPhase === 'BIRD_RECOVER' && Math.abs(this.birdX - BIRD_IDLE_X_POSITION) < 25 && this.phaseElapsedSeconds >= 0.8) {
          this.setPhase('IDLE');
        }
        break;
      }

      case 'PLAYING': {
        // Schwerkraft
        this.birdVelocityY += 600 * deltaSeconds;
        this.birdVelocityY = Math.min(this.birdVelocityY, 520);

        // Rechts-Schub klingt ab
        this.birdVelocityX *= Math.pow(0.08, deltaSeconds);
        this.birdX += this.birdVelocityX * deltaSeconds;
        this.birdY += this.birdVelocityY * deltaSeconds;

        // X innerhalb sinnvoller Grenzen halten
        if (this.birdX < 50)                  this.birdX = 50;
        if (this.birdX > canvasWidth * 0.35)  this.birdX = canvasWidth * 0.35;

        // Rotation folgt Vertikalgeschwindigkeit
        const targetRotation = Math.max(-0.5, Math.min(1.4, this.birdVelocityY / 350));
        this.birdRotation += (targetRotation - this.birdRotation) * 12 * deltaSeconds;

        // Pipes spawnen
        this.pipeSpawnTimer += deltaSeconds;
        if (this.pipeSpawnTimer >= PIPE_SPAWN_INTERVAL_SECONDS) {
          this.pipeSpawnTimer = 0;
          const minGapCenterY = 100 + PIPE_VERTICAL_GAP / 2;
          const maxGapCenterY = canvasHeight - GROUND_HEIGHT - 60 - PIPE_VERTICAL_GAP / 2;
          const gapCenterY = minGapCenterY + Math.random() * (maxGapCenterY - minGapCenterY);
          this.pipes.push({ x: canvasWidth + PIPE_WIDTH, gapCenterY, scored: false });
        }

        // Pipes bewegen + Score zählen
        for (const pipe of this.pipes) {
          pipe.x -= PIPE_SCROLL_SPEED * deltaSeconds;
          if (!pipe.scored && pipe.x + PIPE_WIDTH / 2 < this.birdX) {
            pipe.scored = true;
            this.score++;
          }
        }
        this.pipes = this.pipes.filter(pipe => pipe.x > -PIPE_WIDTH - 10);

        // Kollisionsprüfung
        if (this.checkCollision(canvasHeight)) {
          this.bestScore = Math.max(this.bestScore, this.score);
          this.gameOver.emit(this.score);
          this.setPhase('GAME_OVER');
          this.gameOverElapsedSeconds = 0;
          this.birdVelocityX = -80;
          this.birdVelocityY = -200;
        }
        break;
      }

      case 'GAME_OVER': {
        this.gameOverElapsedSeconds += deltaSeconds;

        // Vogel fällt zu Boden
        this.birdVelocityY += 500 * deltaSeconds;
        this.birdVelocityY = Math.min(this.birdVelocityY, 600);
        this.birdVelocityX *= Math.pow(0.15, deltaSeconds);
        this.birdX += this.birdVelocityX * deltaSeconds;
        this.birdY += this.birdVelocityY * deltaSeconds;
        this.birdRotation += 8 * deltaSeconds;

        const groundRestY = canvasHeight - GROUND_HEIGHT - 18;
        if (this.birdY > groundRestY) {
          this.birdY = groundRestY;
          this.birdVelocityY = 0;
          this.birdVelocityX = 0;
        }
        break;
      }
    }
  }

  private checkCollision(canvasHeight: number): boolean {
    const birdRadius = 13;
    const groundTopY = canvasHeight - GROUND_HEIGHT;

    if (this.birdY + birdRadius >= groundTopY) return true;
    if (this.birdY - birdRadius <= 0)          return true;

    for (const pipe of this.pipes) {
      const pipeLeftX  = pipe.x - PIPE_WIDTH / 2;
      const pipeRightX = pipe.x + PIPE_WIDTH / 2 + 5; // leicht breiter für cap
      const gapTopY    = pipe.gapCenterY - PIPE_VERTICAL_GAP / 2;
      const gapBottomY = pipe.gapCenterY + PIPE_VERTICAL_GAP / 2;

      if (this.birdX + birdRadius > pipeLeftX && this.birdX - birdRadius < pipeRightX) {
        if (this.birdY - birdRadius < gapTopY || this.birdY + birdRadius > gapBottomY) {
          return true;
        }
      }
    }
    return false;
  }

  // ── Draw helpers ──────────────────────────────────────────────

  private drawPipes(canvasHeight: number): void {
    const ctx        = this.canvasContext;
    const groundTopY = canvasHeight - GROUND_HEIGHT;
    const capHeight  = 26;
    const capWidth   = PIPE_WIDTH + 12;

    for (const pipe of this.pipes) {
      const gapTopY    = pipe.gapCenterY - PIPE_VERTICAL_GAP / 2;
      const gapBottomY = pipe.gapCenterY + PIPE_VERTICAL_GAP / 2;

      // --- obere Röhre ---
      // Rohr-Körper
      ctx.fillStyle = '#4ab34a';
      ctx.fillRect(pipe.x - PIPE_WIDTH / 2, 0, PIPE_WIDTH, gapTopY - capHeight);
      // Highlight
      ctx.fillStyle = '#72d572';
      ctx.fillRect(pipe.x - PIPE_WIDTH / 2 + 4, 0, 10, gapTopY - capHeight);
      // Cap
      ctx.fillStyle = '#4ab34a';
      ctx.fillRect(pipe.x - capWidth / 2, gapTopY - capHeight, capWidth, capHeight);
      ctx.fillStyle = '#72d572';
      ctx.fillRect(pipe.x - capWidth / 2 + 4, gapTopY - capHeight, 10, capHeight);
      // Outline
      ctx.strokeStyle = '#2e7d2e';
      ctx.lineWidth = 2;
      ctx.strokeRect(pipe.x - PIPE_WIDTH / 2, 0, PIPE_WIDTH, gapTopY - capHeight);
      ctx.strokeRect(pipe.x - capWidth / 2, gapTopY - capHeight, capWidth, capHeight);

      // --- untere Röhre ---
      const lowerPipeBodyTopY  = gapBottomY + capHeight;
      const lowerPipeBodyHeight = groundTopY - lowerPipeBodyTopY;
      // Cap
      ctx.fillStyle = '#4ab34a';
      ctx.fillRect(pipe.x - capWidth / 2, gapBottomY, capWidth, capHeight);
      ctx.fillStyle = '#72d572';
      ctx.fillRect(pipe.x - capWidth / 2 + 4, gapBottomY, 10, capHeight);
      // Rohr-Körper
      ctx.fillStyle = '#4ab34a';
      ctx.fillRect(pipe.x - PIPE_WIDTH / 2, lowerPipeBodyTopY, PIPE_WIDTH, lowerPipeBodyHeight);
      ctx.fillStyle = '#72d572';
      ctx.fillRect(pipe.x - PIPE_WIDTH / 2 + 4, lowerPipeBodyTopY, 10, lowerPipeBodyHeight);
      // Outline
      ctx.strokeStyle = '#2e7d2e';
      ctx.lineWidth = 2;
      ctx.strokeRect(pipe.x - capWidth / 2, gapBottomY, capWidth, capHeight);
      ctx.strokeRect(pipe.x - PIPE_WIDTH / 2, lowerPipeBodyTopY, PIPE_WIDTH, lowerPipeBodyHeight);
    }
  }

  private drawGround(canvasWidth: number, canvasHeight: number): void {
    const ctx        = this.canvasContext;
    const groundTopY = canvasHeight - GROUND_HEIGHT;
    ctx.fillStyle = '#ded895';
    ctx.fillRect(0, groundTopY, canvasWidth, GROUND_HEIGHT);
    ctx.fillStyle = '#c8b84a';
    ctx.fillRect(0, groundTopY, canvasWidth, 5);
    // Gras
    ctx.fillStyle = '#5a9e3a';
    ctx.fillRect(0, groundTopY - 6, canvasWidth, 8);
  }

  private drawScore(canvasWidth: number): void {
    const ctx        = this.canvasContext;
    const fontFamily = this.isFontLoaded ? '"Press Start 2P"' : '"Courier New", monospace';
    ctx.save();
    ctx.font          = `28px ${fontFamily}`;
    ctx.textAlign     = 'center';
    ctx.textBaseline  = 'top';
    ctx.fillStyle     = '#ffffff';
    ctx.shadowColor   = '#000';
    ctx.shadowBlur    = 0;
    ctx.shadowOffsetX = 2;
    ctx.shadowOffsetY = 2;
    ctx.fillText(`${this.score}`, canvasWidth / 2, 18);
    ctx.restore();
  }

  private drawGameOver(canvasWidth: number, canvasHeight: number): void {
    const ctx        = this.canvasContext;
    const fontFamily = this.isFontLoaded ? '"Press Start 2P"' : '"Courier New", monospace';

    // Panel
    const panelWidth  = canvasWidth * 0.72;
    const panelHeight = canvasHeight * 0.38;
    const panelX      = (canvasWidth - panelWidth) / 2;
    const panelY      = canvasHeight * 0.24;

    ctx.save();
    ctx.fillStyle = 'rgba(0,0,0,0.55)';
    this.roundRect(panelX, panelY, panelWidth, panelHeight, 12);
    ctx.fill();

    ctx.textAlign    = 'center';
    ctx.textBaseline = 'middle';
    ctx.shadowBlur   = 0;

    // GAME OVER
    ctx.font      = `${Math.min(canvasWidth / 11, 30)}px ${fontFamily}`;
    ctx.fillStyle = '#ff4444';
    ctx.shadowOffsetX = 2; ctx.shadowOffsetY = 2;
    ctx.shadowColor = '#000';
    ctx.fillText('GAME OVER', canvasWidth / 2, panelY + panelHeight * 0.22);

    // Scores
    ctx.font      = `${Math.min(canvasWidth / 17, 18)}px ${fontFamily}`;
    ctx.fillStyle = '#ffffff';
    ctx.fillText(`Score: ${this.score}`,     canvasWidth / 2, panelY + panelHeight * 0.48);
    ctx.fillText(`Best:  ${this.bestScore}`, canvasWidth / 2, panelY + panelHeight * 0.68);

    // Restart hint
    if (this.gameOverElapsedSeconds > 1.0) {
      const pulseAlpha = 0.55 + 0.45 * Math.sin(this.gameOverElapsedSeconds * 5);
      ctx.globalAlpha = pulseAlpha;
      ctx.font        = `${Math.min(canvasWidth / 22, 13)}px ${fontFamily}`;
      ctx.fillStyle   = '#aaffaa';
      ctx.shadowOffsetX = 1; ctx.shadowOffsetY = 1;
      ctx.fillText('Click to Restart', canvasWidth / 2, panelY + panelHeight * 0.88);
    }

    ctx.restore();
  }

  private drawStartPrompt(canvasWidth: number, canvasHeight: number): void {
    const ctx        = this.canvasContext;
    const fontFamily = this.isFontLoaded ? '"Press Start 2P"' : '"Courier New", monospace';
    const pulseAlpha = 0.55 + 0.45 * Math.sin(this.phaseElapsedSeconds * 3.5);

    ctx.save();
    ctx.globalAlpha   = pulseAlpha;
    ctx.font          = `${Math.min(canvasWidth / 22, 15)}px ${fontFamily}`;
    ctx.textAlign     = 'center';
    ctx.textBaseline  = 'middle';
    ctx.fillStyle     = '#ffffff';
    ctx.shadowColor   = '#000';
    ctx.shadowOffsetX = 2; ctx.shadowOffsetY = 2;
    ctx.shadowBlur    = 0;
    ctx.fillText('Click to Play!', canvasWidth / 2, canvasHeight * 0.73);
    ctx.restore();
  }

  private roundRect(x: number, y: number, width: number, height: number, cornerRadius: number): void {
    const ctx = this.canvasContext;
    ctx.beginPath();
    ctx.moveTo(x + cornerRadius, y);
    ctx.lineTo(x + width - cornerRadius, y);
    ctx.quadraticCurveTo(x + width, y, x + width, y + cornerRadius);
    ctx.lineTo(x + width, y + height - cornerRadius);
    ctx.quadraticCurveTo(x + width, y + height, x + width - cornerRadius, y + height);
    ctx.lineTo(x + cornerRadius, y + height);
    ctx.quadraticCurveTo(x, y + height, x, y + height - cornerRadius);
    ctx.lineTo(x, y + cornerRadius);
    ctx.quadraticCurveTo(x, y, x + cornerRadius, y);
    ctx.closePath();
  }

  /** Pick the right bird image based on vertical velocity */
  private getBirdFrame(): HTMLImageElement {
    if (this.currentPhase === 'BIRD_TUMBLE' || this.currentPhase === 'GAME_OVER') return this.birdSpriteWingsDown;

    if (this.currentPhase === 'BIRD_FLY_IN' || this.currentPhase === 'IDLE' ||
        this.currentPhase === 'BIRD_RECOVER' || this.currentPhase === 'PLAYING') {
      if (this.birdVelocityY < -40) return this.birdSpriteWingsUp;
      if (this.birdVelocityY >  40) return this.birdSpriteWingsDown;
      return this.birdSpriteWingsMid;
    }

    const flapFrames = [this.birdSpriteWingsUp, this.birdSpriteWingsMid, this.birdSpriteWingsDown, this.birdSpriteWingsMid];
    return flapFrames[this.flapAnimationFrame];
  }

  private spawnTitleExplosionParticles(canvasWidth: number, canvasHeight: number): void {
    const offscreenWidth  = 720;
    const offscreenHeight = 110;
    const offscreenCanvas = document.createElement('canvas');
    offscreenCanvas.width  = offscreenWidth;
    offscreenCanvas.height = offscreenHeight;
    const offscreenContext = offscreenCanvas.getContext('2d')!;
    const fontFamily = this.isFontLoaded ? '"Press Start 2P"' : '"Courier New", monospace';
    offscreenContext.font = `36px ${fontFamily}`;
    offscreenContext.fillStyle = TITLE_GREEN_COLOR;
    offscreenContext.textAlign = 'center';
    offscreenContext.textBaseline = 'middle';
    offscreenContext.fillText('Clappy-Bird', offscreenWidth / 2, offscreenHeight / 2);

    const pixelData = offscreenContext.getImageData(0, 0, offscreenWidth, offscreenHeight).data;
    const particlePixelSize = 5;
    const originX = canvasWidth * TITLE_HORIZONTAL_CENTER_FRACTION - offscreenWidth / 2;
    const originY = canvasHeight * 0.45 - offscreenHeight / 2;

    for (let pixelY = 0; pixelY < offscreenHeight; pixelY += particlePixelSize) {
      for (let pixelX = 0; pixelX < offscreenWidth; pixelX += particlePixelSize) {
        if (pixelData[(pixelY * offscreenWidth + pixelX) * 4 + 3] > 100) {
          this.titleExplosionParticles.push({
            x: originX + pixelX,
            y: originY + pixelY,
            velocityX: (pixelX - offscreenWidth / 2) * 1.0 + (Math.random() - 0.5) * 140,
            velocityY: (Math.random() - 0.5) * 180 - 80,
            alpha: 1,
            size: particlePixelSize,
          });
        }
      }
    }
  }

  private drawBackground(canvasWidth: number, canvasHeight: number): void {
    if (this.backgroundImage.complete && this.backgroundImage.naturalWidth > 0) {
      this.canvasContext.drawImage(this.backgroundImage, this.backgroundScrollX, 0, canvasWidth, canvasHeight);
      this.canvasContext.drawImage(this.backgroundImage, this.backgroundScrollX + canvasWidth, 0, canvasWidth, canvasHeight);
    } else {
      this.canvasContext.clearRect(0, 0, canvasWidth, canvasHeight);
    }
  }

  private drawTitle(canvasWidth: number, canvasHeight: number): void {
    if (
      this.currentPhase !== 'TITLE_ENTER' &&
      this.currentPhase !== 'TITLE_SHOW' &&
      this.currentPhase !== 'BIRD_FLY_IN'
    ) return;

    const ctx        = this.canvasContext;
    const fontFamily = this.isFontLoaded ? '"Press Start 2P"' : '"Courier New", monospace';
    const fontSize   = Math.max(30, Math.min(canvasWidth / 10, 74));

    let titleX = canvasWidth / 2;
    let titleAlpha = 1;

    if (this.currentPhase === 'TITLE_ENTER') {
      const enterProgress = Math.min(this.phaseElapsedSeconds / 1.8, 1);
      const easedProgress = easeOutBack(enterProgress);
      titleX     = canvasWidth / 2 + (1 - easedProgress) * (canvasWidth / 2 + 300);
      titleAlpha = Math.min(enterProgress * 4, 1);
    }

    ctx.save();
    ctx.globalAlpha  = titleAlpha;
    ctx.font         = `${fontSize}px ${fontFamily}`;
    ctx.textAlign    = 'center';
    ctx.textBaseline = 'middle';
    ctx.imageSmoothingEnabled = false;

    // Measure text left edge for collision
    const textWidth = ctx.measureText('Clappy-Bird').width;
    this.titleTextLeftEdge = titleX - textWidth / 2;

    // Drop shadow
    ctx.fillStyle = '#003300';
    ctx.shadowBlur = 0; ctx.shadowOffsetX = 3; ctx.shadowOffsetY = 3;
    ctx.fillText('Clappy-Bird', titleX, canvasHeight * 0.45);

    // Black outline
    ctx.strokeStyle = '#000000';
    ctx.lineWidth = Math.max(3, fontSize * 0.12);
    ctx.lineJoin = 'round';
    ctx.strokeText('Clappy-Bird', titleX, canvasHeight * 0.45);

    // Fill
    ctx.shadowOffsetX = 0; ctx.shadowOffsetY = 0;
    ctx.shadowColor   = 'transparent'; ctx.shadowBlur = 0;
    ctx.fillStyle     = TITLE_GREEN_COLOR;
    ctx.fillText('Clappy-Bird', titleX, canvasHeight * 0.45);

    ctx.restore();
  }

  private drawParticles(): void {
    const ctx = this.canvasContext;
    ctx.save();
    ctx.shadowColor = TITLE_GREEN_COLOR; ctx.shadowBlur = 6;
    ctx.fillStyle   = TITLE_GREEN_COLOR;
    for (const particle of this.titleExplosionParticles) {
      ctx.globalAlpha = Math.max(0, particle.alpha);
      ctx.fillRect(particle.x, particle.y, particle.size, particle.size);
    }
    ctx.restore();
  }

  private drawBird(): void {
    const birdImage = this.getBirdFrame();
    if (!birdImage.complete || !birdImage.naturalWidth) return;

    const birdSpriteWidth  = 48;
    const birdSpriteHeight = 36;
    const ctx = this.canvasContext;
    ctx.save();
    ctx.translate(this.birdX, this.birdY);
    ctx.rotate(this.birdRotation);
    ctx.imageSmoothingEnabled = false;
    ctx.drawImage(birdImage, -birdSpriteWidth / 2, -birdSpriteHeight / 2, birdSpriteWidth, birdSpriteHeight);
    ctx.restore();
  }

  ngOnDestroy(): void {
    cancelAnimationFrame(this.animationFrameId);
    this.resizeObserver?.disconnect();
  }
}
