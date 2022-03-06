const utf8Decoder = new TextDecoder("utf-8");
function assert(value, errorMessage) {if (!value) {throw new Error(errorMessage);}}
function arrayBufferToUtf8(arrayBuffer) {return utf8Decoder.decode(arrayBuffer);}
class BencodeDecoder {
    static DICTIONARY_BEGIN = "d".charCodeAt(0);
    static LIST_BEGIN = "l".charCodeAt(0);
    static INTEGER_BEGIN = "i".charCodeAt(0);
    static TYPE_END = "e".charCodeAt(0);
    static COLON = ":".charCodeAt(0);
    static DIGITS = "0123456789".split("").map(d => d.charCodeAt(0));

    /** @type {DataView} */
    view = null;

    /**
     * @param {DataView} view
     */
    static decode(view) {
        const decoder = new BencodeDecoder(view);
        return decoder.result;
    }

    /**
     * @private
     * @param {DataView} view
     */
    constructor (view) {
        this.view = view;
        this.index = 0;
        this.result = this.parseDictionary();
    }

    /** @private */
    nextByte() {return this.view.getUint8(this.index++);}
    
    /** @private */
    unread() {this.index--;}

    /** @private */
    parseDictionary() {
        const begin = this.nextByte();
        assert(begin === BencodeDecoder.DICTIONARY_BEGIN, "Expected dictionary");

        let dict = {};

        let cmd = this.nextByte();
        while (cmd !== BencodeDecoder.TYPE_END) {
            this.unread();
            const key = this.parseDictionaryKey();
            dict[key] = this.parseValue();

            cmd = this.nextByte();
        }

        return dict;
    }

    /** @private */
    parseValue() {
        const cmd = this.nextByte();
        this.unread();

        switch (cmd) {
            case BencodeDecoder.INTEGER_BEGIN:   return this.parseInteger();
            case BencodeDecoder.LIST_BEGIN:      return this.parseList();
            case BencodeDecoder.DICTIONARY_BEGIN:return this.parseDictionary();
            default:
                if (BencodeDecoder.DIGITS.includes(cmd)) {return this.parseByteString();}
                else {throw new Error(`Unknown value identifier "${cmd}"`);}
        }
    }

    /** @private */
    parseList() {
        assert(this.nextByte() === BencodeDecoder.LIST_BEGIN, "Expected list begin marker");

        const list = [];

        let cmd = this.nextByte();
        while (cmd !== BencodeDecoder.TYPE_END) {
            this.unread();
            list.push(this.parseValue());
            cmd = this.nextByte();
        }

        return list;
    }

    /** @private */
    parseInteger() {
        assert(this.nextByte() === BencodeDecoder.INTEGER_BEGIN, "Expected integer begin marker");
        const integer = this.parseBaseTenNumber();
        assert(this.nextByte() === BencodeDecoder.TYPE_END, "Expected integer end marker");
        return integer;
    }

    /** @private */
    parseDictionaryKey() {return arrayBufferToUtf8(this.parseByteString());}

    /** @private */
    parseByteString() {
        const len = this.parseBaseTenNumber();
        this.matchColon();
        return this.parseString(len);
    }

    /** @private */
    parseBaseTenNumber() {
        let str = "";
        let code = this.nextByte();

        while (BencodeDecoder.DIGITS.includes(code)) {
            str += String.fromCharCode(code);
            code = this.nextByte();
        }
        this.unread();

        return parseInt(str, 10);
    }

    /** @private */
    matchColon() {assert(this.nextByte() === BencodeDecoder.COLON, "Expected colon");}

    /** @private */
    parseString(length) {
        let start = this.index;
        this.index += length;
        return this.view.buffer.slice(start, this.index);
    }
}