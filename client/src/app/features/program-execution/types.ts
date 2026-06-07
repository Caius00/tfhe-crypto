import {FheBool, FheUint8} from "tfhe";

export interface ExecReq {
    server_key: Uint8Array<ArrayBufferLike>,
    cycles: number,
    a: FheUint8,
    b: FheUint8,
    pc: FheUint8,
    carry: FheBool,
    memory: FheUint8[],
}

export interface ExecResp {
    a: FheUint8,
    b: FheUint8,
    pc: FheUint8,
    carry: FheBool,
    memory: FheUint8[],
}
