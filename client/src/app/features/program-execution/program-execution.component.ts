import {ChangeDetectorRef, Component, ElementRef, OnInit, ViewChild} from "@angular/core";
import {ButtonComponent} from "../../shared/components/button/button.component";
import {assemble_code} from "./assemble_code";
import {TfheService} from "../../core/crypto/tfhe.service";
import {InputComponent} from "../../shared/components/input/input.component";
import {FheBool, FheUint8, TfheClientKey, TfheCompressedServerKey} from "tfhe";
import {CheckboxComponent} from "../../shared/components/checkbox/checkbox.component";
import {fromPromise} from "rxjs/internal/observable/innerFrom";
import {HttpClient} from "@angular/common/http";
import {KeyPair} from "../../core/crypto/key-pair.model";
import {ExecReq, ExecResp} from "./types";
import {forkJoin, from, of, switchMap} from "rxjs";
import {getDB, getKey, setKey} from "./db";

@Component({
    selector: "app-program-execution",
    templateUrl: "./program-execution.component.html",
    styleUrls: ["./program-execution.component.scss"],
    imports: [
        ButtonComponent,
        InputComponent,
        CheckboxComponent,
    ]
})
export class ProgramExecutionComponent implements OnInit {
    public programme = "";
    public lineCount = 0;
    public displayMessage = false;
    public message = "";
    public a = "0";
    public b = "0";
    public c = "0";
    public carry = false;
    public cycles = "0";
    private cyclesNum = 0;
    private assembly: number[] = [];
    private aNum = 0;
    private bNum = 0;
    private cNum = 0;
    public assembled = false;
    public addresses = [0];
    public executing = false;
    public generating = false;

    public ram = "";

    public keypair: KeyPair | null = null;

    constructor(private tfhe: TfheService, private http: HttpClient, private cdr: ChangeDetectorRef) {
    }

    ngOnInit(): void {

        this.onTextChange("");
    }

    lines: number[] = [0];

    @ViewChild('textarea') textarea!: ElementRef<HTMLTextAreaElement>;
    @ViewChild('lineCounter') lineCounter!: ElementRef<HTMLDivElement>;

    onTextChange(text: string): void {
        this.programme = text;
        this.lineCount = this.programme.split('\n').length;
        this.lines = Array.from(
            {length: Math.max(1, this.lineCount)},
            (_, i) => i);
        this.assembled = false;
    }

    onScroll(): void {
        if (this.textarea && this.lineCounter) {
            this.lineCounter.nativeElement.scrollTop = this.textarea.nativeElement.scrollTop;
        }
    }

    public invalidate() {
        this.assembled = false;
        this.displayMessage = false;
    }

    public assemble() {
        this.displayMessage = false;
        const assembly = assemble_code(this.programme);
        const a = Number.parseInt(this.a);
        const b = Number.parseInt(this.b);
        const c = Number.parseInt(this.c);
        const cycles = Number.parseInt(this.cycles);

        if (Number.isNaN(a) || a > 255 || Number.isNaN(b) || b > 255 || Number.isNaN(c) || c > 255) {
            this.displayMessage = true;
            this.message = "bad registers";
            return;
        }

        if (Number.isNaN(cycles) || cycles < 0) {
            this.displayMessage = true;
            this.message = "bad cycles";
            return;
        }

        if (
            assembly.some((v) => {
                return Number.isNaN(v) || v < 0;
            })
        ) {
            this.displayMessage = true;
            this.message = "bad programme";
            return;
        }

        this.assembly = assembly;
        this.cyclesNum = cycles;
        this.aNum = a;
        this.bNum = b;
        this.cNum = c;
        this.assembled = true;

        this.ram = assembly.map((n) => n.toString(16)).join("\n");
        this.addresses = Array.from(
            {length: Math.max(1, assembly.length)},
            (_, i) => i);
    }

    public generate() {
        this.generating = true;
        fromPromise(this.tfhe.ensureInitialized())
            .pipe(
                switchMap(() => {
                    return fromPromise(getDB());
                }),
                switchMap((db) => {
                    return forkJoin([from(getKey(db, "ck")), from(getKey(db, "sk")), of(db)]);
                })
            )
            .subscribe(([a, b, db]) => {
                let ck: TfheClientKey | null = null;
                if (!!a) {
                    ck = TfheClientKey.deserialize(this.tfhe.fromBase64(a))
                }
                let sk: Uint8Array | null = null;
                if (!!b) {
                    sk = this.tfhe.fromBase64(b);
                }

                if (!ck || !sk) {
                    if (!!ck) {
                        sk = TfheCompressedServerKey.new(ck).serialize();
                    } else {
                        const p = this.tfhe.generateKeyPair();
                        ck = p.clientKey;
                        sk = p.serverKeyBytes;
                        setKey(db, this.tfhe.toBase64(ck.serialize()), "ck");
                    }

                    setKey(db, this.tfhe.toBase64(sk), "sk");
                }

                this.keypair = {
                    clientKey: ck,
                    serverKeyBytes: sk,
                }

                this.generating = false;
                this.cdr.detectChanges();
            });
    }

    executeStreamingProgram() {
        this.executing = true;
        this.cdr.detectChanges();
        const b64Key = this.tfhe.toBase64(this.keypair!!.serverKeyBytes);
        const b64A = this.tfhe.toBase64(FheUint8.encrypt_with_client_key(this.aNum, this.keypair!!.clientKey).serialize());
        const b64B = this.tfhe.toBase64(FheUint8.encrypt_with_client_key(this.bNum, this.keypair!!.clientKey).serialize());
        const b64Pc = this.tfhe.toBase64(FheUint8.encrypt_with_client_key(this.cNum, this.keypair!!.clientKey).serialize());
        const b64Carry = this.tfhe.toBase64(FheBool.encrypt_with_client_key(this.carry, this.keypair!!.clientKey).serialize());

        const b64Memory = this.assembly.map((v) =>
            this.tfhe.toBase64(FheUint8.encrypt_with_client_key(v, this.keypair!!.clientKey).serialize())
        );

        const req: ExecReq = {
            cycles: this.cyclesNum,
            a: b64A,
            b: b64B,
            pc: b64Pc,
            carry: b64Carry,
            memory: b64Memory,
            server_key: b64Key,
        };

        const ws = new WebSocket("ws://localhost:8080/execute-stream");

        ws.onopen = () => {
            ws.send(JSON.stringify(req));
        };

        ws.onmessage = (event) => {
            const res = JSON.parse(event.data) as ExecResp;
            const ck = this.keypair!!.clientKey;

            const bytesA: Uint8Array = this.tfhe.fromBase64(res.a);
            const bytesB: Uint8Array = this.tfhe.fromBase64(res.b);
            const bytesPc: Uint8Array = this.tfhe.fromBase64(res.pc);
            const bytesCarry = this.tfhe.fromBase64(res.carry);

            const mem: number[] = [];

            for (const cell64 of res.memory) {
                const bytes = this.tfhe.fromBase64(cell64);

                const cell = FheUint8.deserialize(bytes);
                mem.push(cell.decrypt(ck));
            }

            const ddA = FheUint8.deserialize(bytesA);
            const ddB = FheUint8.deserialize(bytesB);
            const ddPc = FheUint8.deserialize(bytesPc);
            const ddCarry = FheBool.deserialize(bytesCarry);

            this.a = ddA.decrypt(ck).toString(16);
            this.b = ddB.decrypt(ck).toString(16);
            this.c = ddPc.decrypt(ck).toString(16);
            this.carry = ddCarry.decrypt(ck);

            this.ram = mem.map((n) => n.toString(16)).join("\n");

            this.cycles = (parseInt(this.cycles) - 1).toString();

            this.cdr.detectChanges();

            this.assembled = true;
        };

        ws.onclose = () => {
            this.executing = false;
            this.cdr.detectChanges();
        };

        ws.onerror = () => {
            this.executing = false;
            this.cdr.detectChanges();
        };
    }

}
