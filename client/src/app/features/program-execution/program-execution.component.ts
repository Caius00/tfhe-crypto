import {Component, ElementRef, OnInit, ViewChild} from "@angular/core";
import {ButtonComponent} from "../../shared/components/button/button.component";
import {assemble_code} from "./assemble_code";
import {TfheService} from "../../core/crypto/tfhe.service";
import {InputComponent} from "../../shared/components/input/input.component";
import {FheBool, FheUint8} from "tfhe";
import {CheckboxComponent} from "../../shared/components/checkbox/checkbox.component";
import {map, switchMap} from "rxjs";
import {fromPromise} from "rxjs/internal/observable/innerFrom";
import {HttpClient} from "@angular/common/http";
import {KeyPair} from "../../core/crypto/key-pair.model";

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

    private keypair?: KeyPair;

    constructor(private tfhe: TfheService, private http: HttpClient) {
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

    }

    onScroll(): void {
        if (this.textarea && this.lineCounter) {
            this.lineCounter.nativeElement.scrollTop = this.textarea.nativeElement.scrollTop;
        }
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

        if (Number.isNaN(cycles)) {
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


        fromPromise(this.tfhe.ensureInitialized())
            .pipe(
                map(() => {
                    if (!this.keypair) {
                        this.keypair = this.tfhe.generateKeyPair();
                    }
                    return this.keypair;
                }),
                switchMap((value) => {
                    const b64Key = this.tfhe.toBase64(value.serverKeyBytes);
                    console.log(value.serverKeyBytes);
                    const b64A = this.tfhe.toBase64(FheUint8.encrypt_with_client_key(a, value.clientKey).serialize());
                    const b64B = this.tfhe.toBase64(FheUint8.encrypt_with_client_key(b, value.clientKey).serialize());
                    const b64Pc = this.tfhe.toBase64(FheUint8.encrypt_with_client_key(c, value.clientKey).serialize());
                    const b64Carry = this.tfhe.toBase64(FheBool.encrypt_with_client_key(this.carry, value.clientKey).serialize());

                    const b64Memory = assembly.map((v) =>
                        this.tfhe.toBase64(FheUint8.encrypt_with_client_key(v, value.clientKey).serialize())
                    );

                    const req = {
                        server_key: b64Key,
                        cycles: cycles,
                        a: b64A,
                        b: b64B,
                        pc: b64Pc,
                        carry: b64Carry,
                        memory: b64Memory
                    };

                    return this.http.post<any>("http://localhost:8080/execute", req);
                })
            )
            .subscribe((res) => {
                const clientKey = this.keypair!!.clientKey;

                const bytesA: Uint8Array = this.tfhe.fromBase64(res.a);
                const bytesB: Uint8Array = this.tfhe.fromBase64(res.b);
                const bytesPc: Uint8Array = this.tfhe.fromBase64(res.pc);

                const finalA = FheUint8.deserialize(bytesA);
                const finalB = FheUint8.deserialize(bytesB);
                const finalPc = FheUint8.deserialize(bytesPc);

                this.a = finalA.decrypt(clientKey).toString(16);
                this.b = finalB.decrypt(clientKey).toString(16);
                this.c = finalPc.decrypt(clientKey).toString(16);

                console.log(this.a, this.b, this.c);
            });
    }
}
