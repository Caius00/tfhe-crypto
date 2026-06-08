import { Component, ElementRef, OnDestroy, AfterViewInit, ViewChild, HostListener, Output, EventEmitter } from '@angular/core';

// Einzelner Pixel der Titel-Explosion
interface ExplosionParticle {
  x: number; y: number;                 // aktuelle Position
  velocityX: number; velocityY: number; // Flugrichtung
  alpha: number; size: number;          // Transparenz + Pixelgröße
}

// Ein Röhrenpaar (oben + unten)
interface Pipe {
  x: number;          // horizontale Position der Mitte
  gapCenterY: number; // Mitte der Lücke
  scored: boolean;    // Punkt bereits vergeben?
}

// Alle Spielzustände vom Intro bis Game Over
type Phase =
  | 'TITLE_ENTER'   // Titel fliegt von rechts ins Bild
  | 'TITLE_SHOW'    // Titel steht kurz still
  | 'BIRD_FLY_IN'   // Vogel fliegt von links auf Titel zu
  | 'COLLISION'     // kurzer Treffer-Moment
  | 'BIRD_TUMBLE'   // Vogel taumelt nach Treffer
  | 'BIRD_RECOVER'  // Vogel fängt sich wieder
  | 'IDLE'          // wartet auf Klick
  | 'PLAYING'       // aktives Spiel
  | 'GAME_OVER';    // Game-Over-Bildschirm

const BACKGROUND_SCROLL_SPEED          = 0.6;   // Pixel pro Frame, Hintergrund nach links
const TITLE_GREEN_COLOR                = '#166534'; // Farbe des "Clappy-Bird"-Titels
const TITLE_HORIZONTAL_CENTER_FRACTION = 0.5;   // Titel horizontal mittig
const PIPE_VERTICAL_GAP                = 165;   // vertikale Lücke zwischen oberer/unterer Röhre
const PIPE_WIDTH                       = 52;    // Breite einer Röhre
const PIPE_SCROLL_SPEED                = 160;   // px/s nach links
const PIPE_SPAWN_INTERVAL_SECONDS      = 2.0;   // Sekunden zwischen neuen Röhrenpaaren
const BIRD_IDLE_X_POSITION             = 90;    // Ruheposition X des Vogels
const GROUND_HEIGHT                    = 58;    // Höhe des Bodenstreifens

// Easing-Kurve mit leichtem Überschwingen (für Titel-Einflug)
function easeOutBack(t: number): number {
  const c1 = 1.70158, c3 = c1 + 1;
  return 1 + c3 * Math.pow(t - 1, 3) + c1 * Math.pow(t - 1, 2);
}

// Bild asynchron laden und sofort zurückgeben (wird beim Zeichnen geprüft)
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
  // Referenz auf <canvas> aus dem Template
  @ViewChild('canvas') canvasRef!: ElementRef<HTMLCanvasElement>;
  // Referenz auf den umgebenden Container (für Größen-Updates)
  @ViewChild('gameWindow') gameWindowRef!: ElementRef<HTMLDivElement>;

  /** Sendet den finalen Score, sobald das Spiel endet. */
  @Output() gameOver = new EventEmitter<number>();

  // 2D-Zeichenkontext des Canvas
  private canvasContext!: CanvasRenderingContext2D;

  // Bild-Assets (Hintergrund + 3 Vogel-Sprites für die Flügelschläge)
  private backgroundImage    = loadImage('/games/flappy-bird/images/background.png');
  private birdSpriteWingsUp  = loadImage('/games/flappy-bird/images/bird/yellowbird-upflap.png');   // Flügel oben
  private birdSpriteWingsMid = loadImage('/games/flappy-bird/images/bird/yellowbird-midflap.png');  // Flügel mittig
  private birdSpriteWingsDown = loadImage('/games/flappy-bird/images/bird/yellowbird-downflap.png');// Flügel unten

  private backgroundScrollX = 0;   // X-Offset für scrollenden Hintergrund
  private animationFrameId = 0;    // Handle für requestAnimationFrame (zum Aufräumen)
  private resizeObserver!: ResizeObserver; // Beobachtet Container-Größe
  private lastFrameTimestamp = 0;  // Zeitstempel des letzten Frames (für deltaSeconds)
  private isFontLoaded = false;    // true, sobald Pixel-Font verfügbar ist

  // Linke Kante des Titel-Textes — pro Frame neu gemessen, für Vogel-Kollision
  private titleTextLeftEdge = 0;

  // Aktuelle Phase + wie lange wir schon drin sind
  private currentPhase: Phase = 'TITLE_ENTER';
  private phaseElapsedSeconds = 0;

  // Vogel-Position, -Geschwindigkeit, -Drehung
  private birdX = -80;          // startet außerhalb des Bildes
  private birdY = 0;
  private birdVelocityX = 0;
  private birdVelocityY = 0;
  private birdRotation = 0;     // Bogenmaß, abhängig von Vertikalgeschwindigkeit

  // Animationszyklus der Flügel (für Intro/Idle)
  private flapAnimationTimer = 0;
  private flapAnimationFrame = 0; // 0=oben 1=mitte 2=unten 3=mitte

  // Partikel der explodierenden Titelschrift
  private titleExplosionParticles: ExplosionParticle[] = [];

  // Spielstatus: aktive Röhren, Spawn-Timer, Punkte, Bestwert, Game-Over-Zeit
  private pipes: Pipe[] = [];
  private pipeSpawnTimer = 0;
  private score = 0;
  private bestScore = 0;
  private gameOverElapsedSeconds = 0;

  // Wird einmal nach dem Rendern aufgerufen — initialisiert Canvas, Resize-Watcher und Loop
  ngAfterViewInit(): void {
    this.canvasContext = this.canvasRef.nativeElement.getContext('2d')!;

    // Canvas an Container-Größe koppeln (z. B. bei Fensteränderung)
    this.resizeObserver = new ResizeObserver(() => this.resizeCanvas());
    this.resizeObserver.observe(this.gameWindowRef.nativeElement);
    this.resizeCanvas();

    // Pixel-Font asynchron laden; bis dahin fallback auf Monospace
    document.fonts.load('16px "Press Start 2P"').then(() => {
      this.isFontLoaded = true;
    });

    // Render-Schleife starten
    this.animationFrameId = requestAnimationFrame((timestamp) => {
      this.lastFrameTimestamp = timestamp;
      this.loop(timestamp);
    });
  }

  // Maus-Klick irgendwo auf der Komponente → Sprung-Input
  @HostListener('click')
  onClick(): void {
    this.handleJumpInput();
  }

  // Leertaste löst ebenfalls Sprung aus (Scroll verhindern)
  @HostListener('window:keydown', ['$event'])
  onKeyDown(event: KeyboardEvent): void {
    if (event.code !== 'Space' && event.key !== ' ') return;
    event.preventDefault();
    this.handleJumpInput();
  }

  // Was passieren soll, hängt von der Phase ab: starten, flappen oder neu starten
  private handleJumpInput(): void {
    if (this.currentPhase === 'IDLE') {
      this.startGame();
    } else if (this.currentPhase === 'PLAYING') {
      this.flap();
    } else if (this.currentPhase === 'GAME_OVER' && this.gameOverElapsedSeconds > 1.0) {
      // kleine Verzögerung, damit man nicht versehentlich sofort neu startet
      this.startGame();
    }
  }

  // Spielzustand zurücksetzen und in PLAYING-Phase wechseln
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

  // Ein "Sprung": Vogel bekommt Aufwärtsimpuls und leichten Schwung nach rechts
  private flap(): void {
    this.birdVelocityY = -400;
    this.birdVelocityX = Math.min(this.birdVelocityX + 45, 90); // kleiner Rechts-Schub
  }

  // Canvas-Auflösung an die Container-Größe anpassen
  private resizeCanvas(): void {
    const containerElement = this.gameWindowRef.nativeElement;
    const canvasElement    = this.canvasRef.nativeElement;
    canvasElement.width  = containerElement.clientWidth;
    canvasElement.height = containerElement.clientHeight;
  }

  // Haupt-Render-Loop: pro Frame Logik aktualisieren + alles neu zeichnen
  private loop(timestamp: number): void {
    // Zeit seit letztem Frame (auf max. 50 ms gedeckelt gegen Tab-Pausen)
    const deltaSeconds = Math.min((timestamp - this.lastFrameTimestamp) / 1000, 0.05);
    this.lastFrameTimestamp = timestamp;
    this.phaseElapsedSeconds += deltaSeconds;

    const { width: canvasWidth, height: canvasHeight } = this.canvasRef.nativeElement;

    // Hintergrund weiter nach links scrollen, am Rand zurücksetzen (Endlosschleife)
    this.backgroundScrollX -= BACKGROUND_SCROLL_SPEED;
    if (this.backgroundScrollX <= -canvasWidth) this.backgroundScrollX = 0;

    this.drawBackground(canvasWidth, canvasHeight);
    this.update(canvasWidth, canvasHeight, deltaSeconds); // Physik / Phasenlogik

    // Pipes & Ground nur im Spiel
    if (this.currentPhase === 'PLAYING' || this.currentPhase === 'GAME_OVER') {
      this.drawPipes(canvasHeight);
      this.drawGround(canvasWidth, canvasHeight);
    }

    this.drawTitle(canvasWidth, canvasHeight); // intern: nur in Titel-Phasen sichtbar
    this.drawParticles();                       // Titel-Explosionspartikel

    // Vogel in allen Phasen außer den reinen Titel-Phasen zeichnen
    if (this.currentPhase !== 'TITLE_ENTER' && this.currentPhase !== 'TITLE_SHOW') {
      this.drawBird();
    }

    // Score nur während des Spiels einblenden
    if (this.currentPhase === 'PLAYING') {
      this.drawScore(canvasWidth);
    }

    // Game-Over-Overlay über Boden + Score zeichnen
    if (this.currentPhase === 'GAME_OVER') {
      this.drawGround(canvasWidth, canvasHeight);
      this.drawScore(canvasWidth);
      this.drawGameOver(canvasWidth, canvasHeight);
    }

    // "Click to Play"-Hinweis im Idle-Modus
    if (this.currentPhase === 'IDLE') {
      this.drawStartPrompt(canvasWidth, canvasHeight);
    }

    // Nächster Frame
    this.animationFrameId = requestAnimationFrame((nextTimestamp) => this.loop(nextTimestamp));
  }

  // Phase wechseln und Timer zurücksetzen
  private setPhase(nextPhase: Phase): void {
    this.currentPhase = nextPhase;
    this.phaseElapsedSeconds = 0;
  }

  // Spielwelt-Update für einen Frame (Physik + Phasenautomat)
  private update(canvasWidth: number, canvasHeight: number, deltaSeconds: number): void {
    const verticalCenter = canvasHeight * 0.45; // Mittelhöhe für Ruheflug

    // Flügelschlag-Frame zyklisch weiterschalten (alle 110 ms)
    this.flapAnimationTimer += deltaSeconds;
    if (this.flapAnimationTimer > 0.11) {
      this.flapAnimationFrame = (this.flapAnimationFrame + 1) % 4;
      this.flapAnimationTimer = 0;
    }

    switch (this.currentPhase) {

      // Titel fliegt von rechts ins Bild — nur Timer prüfen
      case 'TITLE_ENTER':
        if (this.phaseElapsedSeconds >= 1.8) this.setPhase('TITLE_SHOW');
        break;

      // Titel steht still, dann Vogel-Einflug starten
      case 'TITLE_SHOW':
        if (this.phaseElapsedSeconds >= 1.0) {
          this.setPhase('BIRD_FLY_IN');
          this.birdX = -80;                // außerhalb des Bildes
          this.birdY = verticalCenter;
          this.birdVelocityX = 440;        // schnell nach rechts
          this.birdVelocityY = 0;
        }
        break;

      // Vogel fliegt mit Wellenbewegung auf Titel zu, Kollision = Explosion
      case 'BIRD_FLY_IN':
        this.birdX += this.birdVelocityX * deltaSeconds;
        this.birdY = verticalCenter + Math.sin(this.phaseElapsedSeconds * 9) * 10; // sanftes Wippen
        this.birdVelocityY = Math.cos(this.phaseElapsedSeconds * 9) * 90;
        this.birdRotation = -0.15;
        // Kollision mit linker Kante der Titelschrift?
        if (this.titleTextLeftEdge > 0 && this.birdX + 24 >= this.titleTextLeftEdge) {
          this.birdX = this.titleTextLeftEdge - 24;
          this.setPhase('COLLISION');
          this.spawnTitleExplosionParticles(canvasWidth, canvasHeight);
          this.birdVelocityX = -380;       // Rückprall nach links
          this.birdVelocityY = -220;       // Schub nach oben
        }
        break;

      // Sehr kurzer "Hit"-Moment (Hitstop), bevor das Taumeln beginnt
      case 'COLLISION':
        if (this.phaseElapsedSeconds >= 0.06) this.setPhase('BIRD_TUMBLE');
        break;

      // Vogel taumelt, Partikel fliegen + verblassen
      case 'BIRD_TUMBLE':
        this.birdX += this.birdVelocityX * deltaSeconds;
        this.birdY += this.birdVelocityY * deltaSeconds;
        this.birdVelocityY += 420 * deltaSeconds;              // Schwerkraft
        this.birdVelocityX += (0 - this.birdVelocityX) * 2 * deltaSeconds; // X bremst aus
        this.birdRotation += 9 * deltaSeconds;                 // dreht sich
        for (const particle of this.titleExplosionParticles) {
          particle.x += particle.velocityX * deltaSeconds;
          particle.y += particle.velocityY * deltaSeconds;
          particle.velocityY += 280 * deltaSeconds; // Partikel fallen
          particle.alpha -= deltaSeconds * 1.0;     // und verblassen
        }
        // unsichtbare Partikel entfernen
        this.titleExplosionParticles = this.titleExplosionParticles.filter((particle) => particle.alpha > 0);
        if (this.phaseElapsedSeconds >= 1.6) this.setPhase('BIRD_RECOVER');
        break;

      // Vogel fängt sich wieder und schwebt zur Idle-Position; Idle = wartet auf Klick
      case 'BIRD_RECOVER':
      case 'IDLE': {
        this.birdVelocityY += 380 * deltaSeconds;     // Schwerkraft
        this.birdVelocityY = Math.min(this.birdVelocityY, 400); // Fallgeschwindigkeit gedeckelt

        // Auto-Flap, damit der Vogel im Idle nicht abstürzt
        if (this.birdY >= verticalCenter + 100 && this.birdVelocityY > 0) {
          this.birdVelocityY = -390;
        }

        // X-Position sanft zur Idle-X ziehen + dämpfen
        this.birdVelocityX += (BIRD_IDLE_X_POSITION - this.birdX) * 4 * deltaSeconds;
        this.birdVelocityX *= Math.pow(0.05, deltaSeconds);

        this.birdX += this.birdVelocityX * deltaSeconds;
        this.birdY += this.birdVelocityY * deltaSeconds;

        // Rotation an Vertikalgeschwindigkeit anpassen (geclamped)
        const targetRotation = Math.max(-0.5, Math.min(1.2, this.birdVelocityY / 300));
        this.birdRotation += (targetRotation - this.birdRotation) * 10 * deltaSeconds;

        // Wenn Vogel nahe Idle-Position ist → in IDLE wechseln
        if (this.currentPhase === 'BIRD_RECOVER' && Math.abs(this.birdX - BIRD_IDLE_X_POSITION) < 25 && this.phaseElapsedSeconds >= 0.8) {
          this.setPhase('IDLE');
        }
        break;
      }

      // Aktives Spiel: Physik, Röhren spawnen/bewegen, Kollisionen prüfen
      case 'PLAYING': {
        // Schwerkraft
        this.birdVelocityY += 600 * deltaSeconds;
        this.birdVelocityY = Math.min(this.birdVelocityY, 520); // Endgeschwindigkeit deckeln

        // Rechts-Schub klingt ab (Reibung)
        this.birdVelocityX *= Math.pow(0.08, deltaSeconds);
        this.birdX += this.birdVelocityX * deltaSeconds;
        this.birdY += this.birdVelocityY * deltaSeconds;

        // X innerhalb sinnvoller Grenzen halten (nicht zu nah am Rand)
        if (this.birdX < 50)                  this.birdX = 50;
        if (this.birdX > canvasWidth * 0.35)  this.birdX = canvasWidth * 0.35;

        // Rotation folgt Vertikalgeschwindigkeit (geclamped)
        const targetRotation = Math.max(-0.5, Math.min(1.4, this.birdVelocityY / 350));
        this.birdRotation += (targetRotation - this.birdRotation) * 12 * deltaSeconds;

        // Pipes spawnen — alle PIPE_SPAWN_INTERVAL_SECONDS ein neues Paar mit zufälliger Lücke
        this.pipeSpawnTimer += deltaSeconds;
        if (this.pipeSpawnTimer >= PIPE_SPAWN_INTERVAL_SECONDS) {
          this.pipeSpawnTimer = 0;
          // erlaubter Bereich für die Lückenmitte (Abstand zu Decke + Boden)
          const minGapCenterY = 100 + PIPE_VERTICAL_GAP / 2;
          const maxGapCenterY = canvasHeight - GROUND_HEIGHT - 60 - PIPE_VERTICAL_GAP / 2;
          const gapCenterY = minGapCenterY + Math.random() * (maxGapCenterY - minGapCenterY);
          this.pipes.push({ x: canvasWidth + PIPE_WIDTH, gapCenterY, scored: false });
        }

        // Pipes bewegen + Score zählen, sobald Vogel vorbei ist
        for (const pipe of this.pipes) {
          pipe.x -= PIPE_SCROLL_SPEED * deltaSeconds;
          if (!pipe.scored && pipe.x + PIPE_WIDTH / 2 < this.birdX) {
            pipe.scored = true;
            this.score++;
          }
        }
        // Pipes, die ganz links aus dem Bild gescrollt sind, entfernen
        this.pipes = this.pipes.filter(pipe => pipe.x > -PIPE_WIDTH - 10);

        // Kollisionsprüfung — bei Treffer ins Game-Over wechseln
        if (this.checkCollision(canvasHeight)) {
          this.bestScore = Math.max(this.bestScore, this.score);
          this.gameOver.emit(this.score); // Score nach außen melden
          this.setPhase('GAME_OVER');
          this.gameOverElapsedSeconds = 0;
          this.birdVelocityX = -80;       // kleiner Rückprall
          this.birdVelocityY = -200;      // kurz nach oben, bevor er fällt
        }
        break;
      }

      // Vogel fällt zu Boden und bleibt liegen
      case 'GAME_OVER': {
        this.gameOverElapsedSeconds += deltaSeconds;

        // Vogel fällt zu Boden
        this.birdVelocityY += 500 * deltaSeconds;
        this.birdVelocityY = Math.min(this.birdVelocityY, 600);
        this.birdVelocityX *= Math.pow(0.15, deltaSeconds);
        this.birdX += this.birdVelocityX * deltaSeconds;
        this.birdY += this.birdVelocityY * deltaSeconds;
        this.birdRotation += 8 * deltaSeconds;

        // Auf dem Boden "stoppen"
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

  // Prüft Boden-, Decken- und Röhren-Kollision für den Vogel (Kreis vs. Rechteck)
  private checkCollision(canvasHeight: number): boolean {
    const birdRadius = 13;                              // angenommener Vogel-Radius
    const groundTopY = canvasHeight - GROUND_HEIGHT;

    if (this.birdY + birdRadius >= groundTopY) return true; // Boden
    if (this.birdY - birdRadius <= 0)          return true; // Decke

    for (const pipe of this.pipes) {
      const pipeLeftX  = pipe.x - PIPE_WIDTH / 2;
      const pipeRightX = pipe.x + PIPE_WIDTH / 2 + 5;  // leicht breiter für cap
      const gapTopY    = pipe.gapCenterY - PIPE_VERTICAL_GAP / 2;
      const gapBottomY = pipe.gapCenterY + PIPE_VERTICAL_GAP / 2;

      // Vogel horizontal in Röhre? Dann auf vertikale Lücke prüfen
      if (this.birdX + birdRadius > pipeLeftX && this.birdX - birdRadius < pipeRightX) {
        if (this.birdY - birdRadius < gapTopY || this.birdY + birdRadius > gapBottomY) {
          return true; // außerhalb der Lücke = Treffer
        }
      }
    }
    return false;
  }

  // ── Draw helpers ──────────────────────────────────────────────

  // Zeichnet alle aktiven Röhrenpaare (Körper + Highlight + Cap + Outline)
  private drawPipes(canvasHeight: number): void {
    const ctx        = this.canvasContext;
    const groundTopY = canvasHeight - GROUND_HEIGHT;
    const capHeight  = 26;                // Höhe der Röhren-"Mündung"
    const capWidth   = PIPE_WIDTH + 12;   // Cap ist etwas breiter als der Schaft

    for (const pipe of this.pipes) {
      const gapTopY    = pipe.gapCenterY - PIPE_VERTICAL_GAP / 2; // untere Kante der oberen Röhre
      const gapBottomY = pipe.gapCenterY + PIPE_VERTICAL_GAP / 2; // obere Kante der unteren Röhre

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

  // Zeichnet den Boden: Sandfläche, dunkler Streifen oben + Gras
  private drawGround(canvasWidth: number, canvasHeight: number): void {
    const ctx        = this.canvasContext;
    const groundTopY = canvasHeight - GROUND_HEIGHT;
    ctx.fillStyle = '#ded895';                              // Sand
    ctx.fillRect(0, groundTopY, canvasWidth, GROUND_HEIGHT);
    ctx.fillStyle = '#c8b84a';                              // dunkler Streifen
    ctx.fillRect(0, groundTopY, canvasWidth, 5);
    // Gras
    ctx.fillStyle = '#5a9e3a';
    ctx.fillRect(0, groundTopY - 6, canvasWidth, 8);
  }

  // Zeichnet die aktuelle Punktzahl mittig oben am Bildschirm
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
    ctx.shadowOffsetX = 2;                                   // schwarzer Versatz-Schatten
    ctx.shadowOffsetY = 2;
    ctx.fillText(`${this.score}`, canvasWidth / 2, 18);
    ctx.restore();
  }

  // Zeichnet das Game-Over-Panel: Hintergrund, Überschrift, Scores, Restart-Hinweis
  private drawGameOver(canvasWidth: number, canvasHeight: number): void {
    const ctx        = this.canvasContext;
    const fontFamily = this.isFontLoaded ? '"Press Start 2P"' : '"Courier New", monospace';

    // Panel-Maße (zentriert)
    const panelWidth  = canvasWidth * 0.72;
    const panelHeight = canvasHeight * 0.38;
    const panelX      = (canvasWidth - panelWidth) / 2;
    const panelY      = canvasHeight * 0.24;

    ctx.save();
    // halbtransparenter Hintergrund mit abgerundeten Ecken
    ctx.fillStyle = 'rgba(0,0,0,0.55)';
    this.roundRect(panelX, panelY, panelWidth, panelHeight, 12);
    ctx.fill();

    ctx.textAlign    = 'center';
    ctx.textBaseline = 'middle';
    ctx.shadowBlur   = 0;

    // GAME OVER (rote Überschrift)
    ctx.font      = `${Math.min(canvasWidth / 11, 30)}px ${fontFamily}`;
    ctx.fillStyle = '#ff4444';
    ctx.shadowOffsetX = 2; ctx.shadowOffsetY = 2;
    ctx.shadowColor = '#000';
    ctx.fillText('GAME OVER', canvasWidth / 2, panelY + panelHeight * 0.22);

    // Aktueller Score + Bestwert
    ctx.font      = `${Math.min(canvasWidth / 17, 18)}px ${fontFamily}`;
    ctx.fillStyle = '#ffffff';
    ctx.fillText(`Score: ${this.score}`,     canvasWidth / 2, panelY + panelHeight * 0.48);
    ctx.fillText(`Best:  ${this.bestScore}`, canvasWidth / 2, panelY + panelHeight * 0.68);

    // Pulsierender "Click to Restart"-Hinweis nach 1 s
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

  // Pulsierender "Click to Play"-Hinweis im Idle-Zustand
  private drawStartPrompt(canvasWidth: number, canvasHeight: number): void {
    const ctx        = this.canvasContext;
    const fontFamily = this.isFontLoaded ? '"Press Start 2P"' : '"Courier New", monospace';
    const pulseAlpha = 0.55 + 0.45 * Math.sin(this.phaseElapsedSeconds * 3.5); // Pulsieren

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

  // Hilfspfad: Rechteck mit abgerundeten Ecken (für Game-Over-Panel)
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

  /** Wählt das passende Vogel-Sprite je nach Phase und Vertikalgeschwindigkeit. */
  private getBirdFrame(): HTMLImageElement {
    // Beim Sturz immer "Flügel unten"
    if (this.currentPhase === 'BIRD_TUMBLE' || this.currentPhase === 'GAME_OVER') return this.birdSpriteWingsDown;

    // In Flug-/Spiel-Phasen: Frame an Vertikalgeschwindigkeit koppeln (steigt/fällt/neutral)
    if (this.currentPhase === 'BIRD_FLY_IN' || this.currentPhase === 'IDLE' ||
        this.currentPhase === 'BIRD_RECOVER' || this.currentPhase === 'PLAYING') {
      if (this.birdVelocityY < -40) return this.birdSpriteWingsUp;
      if (this.birdVelocityY >  40) return this.birdSpriteWingsDown;
      return this.birdSpriteWingsMid;
    }

    // Fallback: zyklische Flügel-Animation
    const flapFrames = [this.birdSpriteWingsUp, this.birdSpriteWingsMid, this.birdSpriteWingsDown, this.birdSpriteWingsMid];
    return flapFrames[this.flapAnimationFrame];
  }

  // Erzeugt aus dem gerasterten Titel-Text einzelne Pixel-Partikel für die Explosion
  private spawnTitleExplosionParticles(canvasWidth: number, canvasHeight: number): void {
    // Off-Screen-Canvas: hier wird der Titeltext einmal "gerendert" um pro Pixel zu sampeln
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

    // Rohe RGBA-Pixel des Off-Screen-Canvas auslesen
    const pixelData = offscreenContext.getImageData(0, 0, offscreenWidth, offscreenHeight).data;
    const particlePixelSize = 5; // Schrittweite = Partikelgröße (gröberes Sampling)
    // Linke obere Ecke, an der die Partikel im sichtbaren Canvas erscheinen sollen
    const originX = canvasWidth * TITLE_HORIZONTAL_CENTER_FRACTION - offscreenWidth / 2;
    const originY = canvasHeight * 0.45 - offscreenHeight / 2;

    // Über alle Sample-Pixel iterieren und nur sichtbare Stellen (Alpha > 100) in Partikel verwandeln
    for (let pixelY = 0; pixelY < offscreenHeight; pixelY += particlePixelSize) {
      for (let pixelX = 0; pixelX < offscreenWidth; pixelX += particlePixelSize) {
        if (pixelData[(pixelY * offscreenWidth + pixelX) * 4 + 3] > 100) {
          this.titleExplosionParticles.push({
            x: originX + pixelX,
            y: originY + pixelY,
            // X-Geschwindigkeit: von der Mitte weg + leichte Zufallsstreuung
            velocityX: (pixelX - offscreenWidth / 2) * 1.0 + (Math.random() - 0.5) * 140,
            // Y-Geschwindigkeit: nach oben + Streuung
            velocityY: (Math.random() - 0.5) * 180 - 80,
            alpha: 1,
            size: particlePixelSize,
          });
        }
      }
    }
  }

  // Endlos scrollender Hintergrund: 2 Kopien des Bildes nebeneinander
  private drawBackground(canvasWidth: number, canvasHeight: number): void {
    if (this.backgroundImage.complete && this.backgroundImage.naturalWidth > 0) {
      this.canvasContext.drawImage(this.backgroundImage, this.backgroundScrollX, 0, canvasWidth, canvasHeight);
      this.canvasContext.drawImage(this.backgroundImage, this.backgroundScrollX + canvasWidth, 0, canvasWidth, canvasHeight);
    } else {
      // Bild noch nicht geladen → einfach leer rendern
      this.canvasContext.clearRect(0, 0, canvasWidth, canvasHeight);
    }
  }

  // Zeichnet den "Clappy-Bird"-Titel mit Schatten, Outline, Füllung + ggf. Einflug-Animation
  private drawTitle(canvasWidth: number, canvasHeight: number): void {
    // Titel ist nur in den Intro-Phasen sichtbar — sonst direkt raus
    if (
      this.currentPhase !== 'TITLE_ENTER' &&
      this.currentPhase !== 'TITLE_SHOW' &&
      this.currentPhase !== 'BIRD_FLY_IN'
    ) return;

    const ctx        = this.canvasContext;
    const fontFamily = this.isFontLoaded ? '"Press Start 2P"' : '"Courier New", monospace';
    const fontSize   = Math.max(30, Math.min(canvasWidth / 10, 74)); // responsiv

    let titleX = canvasWidth / 2;
    let titleAlpha = 1;

    // Einflug von rechts mit easeOutBack + Fade-In
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
    ctx.imageSmoothingEnabled = false; // scharfe Pixelkanten

    // Linke Textkante messen — wird vom Vogel-Einflug für Kollision genutzt
    const textWidth = ctx.measureText('Clappy-Bird').width;
    this.titleTextLeftEdge = titleX - textWidth / 2;

    // Schlagschatten (versetzte dunkle Kopie)
    ctx.fillStyle = '#003300';
    ctx.shadowBlur = 0; ctx.shadowOffsetX = 3; ctx.shadowOffsetY = 3;
    ctx.fillText('Clappy-Bird', titleX, canvasHeight * 0.45);

    // Schwarze Outline
    ctx.strokeStyle = '#000000';
    ctx.lineWidth = Math.max(3, fontSize * 0.12);
    ctx.lineJoin = 'round';
    ctx.strokeText('Clappy-Bird', titleX, canvasHeight * 0.45);

    // Grüne Füllung
    ctx.shadowOffsetX = 0; ctx.shadowOffsetY = 0;
    ctx.shadowColor   = 'transparent'; ctx.shadowBlur = 0;
    ctx.fillStyle     = TITLE_GREEN_COLOR;
    ctx.fillText('Clappy-Bird', titleX, canvasHeight * 0.45);

    ctx.restore();
  }

  // Zeichnet die Explosionspartikel (grün leuchtende Quadrate)
  private drawParticles(): void {
    const ctx = this.canvasContext;
    ctx.save();
    ctx.shadowColor = TITLE_GREEN_COLOR; ctx.shadowBlur = 6; // Glow-Effekt
    ctx.fillStyle   = TITLE_GREEN_COLOR;
    for (const particle of this.titleExplosionParticles) {
      ctx.globalAlpha = Math.max(0, particle.alpha);
      ctx.fillRect(particle.x, particle.y, particle.size, particle.size);
    }
    ctx.restore();
  }

  // Zeichnet den Vogel an aktueller Position, gedreht um birdRotation
  private drawBird(): void {
    const birdImage = this.getBirdFrame();
    if (!birdImage.complete || !birdImage.naturalWidth) return; // Sprite noch nicht geladen

    const birdSpriteWidth  = 48;
    const birdSpriteHeight = 36;
    const ctx = this.canvasContext;
    ctx.save();
    ctx.translate(this.birdX, this.birdY);     // Ursprung = Vogelmitte
    ctx.rotate(this.birdRotation);
    ctx.imageSmoothingEnabled = false;         // scharfe Pixel
    ctx.drawImage(birdImage, -birdSpriteWidth / 2, -birdSpriteHeight / 2, birdSpriteWidth, birdSpriteHeight);
    ctx.restore();
  }

  // Aufräumen: Animationsschleife stoppen + Resize-Beobachter trennen
  ngOnDestroy(): void {
    cancelAnimationFrame(this.animationFrameId);
    this.resizeObserver?.disconnect();
  }
}
