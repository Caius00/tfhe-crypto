export interface ExecReq {
    cycles: number,
    a: string,
    b: string,
    pc: string,
    carry: string,
    memory: string[],
    server_key: string,
}

export interface ExecResp {
    a: string,
    b: string,
    pc: string,
    carry: string,
    memory: string[],
}
