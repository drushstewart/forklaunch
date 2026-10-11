/** Pull-driven SSE decoding; iterator completion or cancellation releases the reader. */
function sseEvents(response: Response, maxEventBytes = 1048576): AsyncGenerator<{
    data: unknown;
    id?: string;
}> {
    if (!response.body)
        throw new TypeError('No response body for event stream');
    const reader = response.body.getReader();
    const decoder = new TextDecoder('utf-8', { ignoreBOM: true });
    let line: number[] = [];
    let data = '';
    let id: string | undefined;
    let skipLf = false;
    let firstLine = true;
    let frameBytes = 0;
    let released = false;
    async function close() {
        if (released)
            return;
        released = true;
        try {
            await reader.cancel();
        }
        catch { /* Preserve the original read/parse error. */ }
        finally {
            reader.releaseLock();
        }
    }
    async function* decode(): AsyncGenerator<{
        data: unknown;
        id?: string;
    }> {
        try {
            if (!Number.isSafeInteger(maxEventBytes) || maxEventBytes < 1)
                throw new RangeError('SSE event limit must be positive');
            while (true) {
                const { done, value } = await reader.read();
                if (done)
                    break;
                for (const byte of value) {
                    if (skipLf) {
                        skipLf = false;
                        if (byte === 10)
                            continue;
                    }
                    if (++frameBytes > maxEventBytes)
                        throw new RangeError('SSE event exceeds configured limit');
                    if (byte !== 10 && byte !== 13) {
                        line.push(byte);
                        continue;
                    }
                    skipLf = byte === 13;
                    let text = decoder.decode(Uint8Array.from(line));
                    line = [];
                    if (firstLine) {
                        firstLine = false;
                        if (text.startsWith('\uFEFF'))
                            text = text.slice(1);
                    }
                    if (text === '') {
                        frameBytes = 0;
                        if (data !== '') {
                            const raw = data.slice(0, -1);
                            data = '';
                            let parsed: unknown;
                            try {
                                parsed = JSON.parse(raw);
                            }
                            catch {
                                parsed = raw;
                            }
                            yield id === undefined ? { data: parsed } : { data: parsed, id };
                        }
                        continue;
                    }
                    if (text.startsWith(':'))
                        continue;
                    const colon = text.indexOf(':');
                    const field = colon < 0 ? text : text.slice(0, colon);
                    let fieldValue = colon < 0 ? '' : text.slice(colon + 1);
                    if (fieldValue.startsWith(' '))
                        fieldValue = fieldValue.slice(1);
                    if (field === 'data')
                        data += `${fieldValue}\n`;
                    else if (field === 'id' && !fieldValue.includes('\0'))
                        id = fieldValue;
                }
            }
        }
        finally {
            await close();
        }
    }
    const iterator = decode();
    const finish = iterator.return.bind(iterator);
    const fail = iterator.throw.bind(iterator);
    iterator.return = async (value) => { await close(); return finish(value); };
    iterator.throw = async (error) => { await close(); return fail(error); };
    return iterator;
}
