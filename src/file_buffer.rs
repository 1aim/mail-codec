use std::ops::Deref;

use mime::{TEXT, CHARSET};

use mheaders::components::{TransferEncoding, MediaType};
use core::utils::FileMeta;
use core::error::{ErrorKind, Result};
use core::codec::{quoted_printable, base64};



// WHEN_FEATURE(more_charsets)
// for now this is just a vector,
// but when <encodings> is used to support
// non-utf8/non-ascii encodings this will
// have more fields, like e.g. `encoding: EncodingSpec`
#[derive(Debug, Clone)]
pub struct FileBuffer {
    content_type: MediaType,
    data: Vec<u8>,
    file_meta: FileMeta
}


impl FileBuffer {

    pub fn new( content_type: MediaType, data: Vec<u8> ) -> FileBuffer {
        FileBuffer::new_with_file_meta( content_type, data, Default::default() )
    }

    pub fn new_with_file_meta( content_type: MediaType, data: Vec<u8>, file_meta: FileMeta ) -> FileBuffer {
        FileBuffer { content_type, data, file_meta }
    }

    pub fn with_data<FN>( mut self, modif: FN ) -> Self
        where FN: FnOnce( Vec<u8> ) -> Vec<u8>
    {
        self.data = modif( self.data );
        self
    }

    pub fn content_type( &self ) -> &MediaType {
        &self.content_type
    }

    pub fn file_meta( &self ) -> &FileMeta {
        &self.file_meta
    }

    pub fn file_meta_mut( &mut self ) -> &mut FileMeta {
        &mut self.file_meta
    }

    pub fn has_ascii_charset( &self ) -> bool {
        let ct = self.content_type();
        ct.type_() == TEXT &&
            ct.get_param(CHARSET)
                .map(|charset| charset == "us-ascii")
                .unwrap_or(true)
    }

    pub fn contains_text( &self ) -> bool {
        let type_ = self.content_type().type_();
        type_ == TEXT
    }

}

impl Deref for FileBuffer {
    type Target = [u8];
    fn deref( &self ) -> &[u8] {
        &*self.data
    }
}

impl Into< Vec<u8> > for FileBuffer {
    fn into(self) -> Vec<u8> {
        self.data
    }
}




pub fn find_encoding( buffer: &FileBuffer ) -> TransferEncoding {
    if buffer.has_ascii_charset() {
        //TODO support lossy 7Bit encoding dropping '\0' and orphan '\n', '\r'
        TransferEncoding::_7Bit
    } else if buffer.contains_text() {
        TransferEncoding::QuotedPrintable
    } else {
        TransferEncoding::Base64
    }
}


#[derive(Debug, Clone)]
pub struct TransferEncodedFileBuffer {
    inner: FileBuffer,
    encoding: TransferEncoding
}

impl TransferEncodedFileBuffer {
    pub fn buffer_is_encoded( buf: FileBuffer, with_encoding: TransferEncoding ) -> Self {
        TransferEncodedFileBuffer {
            inner: buf,
            encoding: with_encoding
        }
    }

    pub fn transfer_encoding( &self ) -> &TransferEncoding {
        &self.encoding
    }

    /// transforms a unencoded FileBuffer into a TransferEncodedFileBuffer
///
/// if a preferred_encoding is given it is used,
/// else if the buffer has a ascii charset 7Bit encoding is used
/// else if the buffer contains text quoted-printable is used
/// else base64 encoding is used
    pub fn encode_buffer(
        buffer: FileBuffer,
        preferred_encoding: Option<&TransferEncoding>
    ) -> Result<TransferEncodedFileBuffer>
    {
        use self::TransferEncoding::*;
        let encoding;
        let encoding_ref;

        if let Some( enc_ref ) = preferred_encoding {
            encoding_ref = enc_ref;
        } else {
            encoding = find_encoding( &buffer );
            encoding_ref = &encoding;
        }

        match *encoding_ref {
            _7Bit => encode_7bit( buffer ),
            _8Bit => encode_8bit( buffer ),
            Binary => encode_binary( buffer ),
            QuotedPrintable => encode_quoted_printable( buffer ),
            Base64 => encode_base64( buffer ),
        }
    }

}


impl Deref for TransferEncodedFileBuffer {
    type Target = FileBuffer;
    fn deref( &self ) -> &FileBuffer {
        &self.inner
    }
}



fn encode_7bit( buffer: FileBuffer ) -> Result<TransferEncodedFileBuffer> {
    {
        let data: &[u8] = &*buffer;

        let mut last = b'\0';
        for byte in data.iter().cloned() {
            if byte >= 128 || byte == 0 {
                return Err( ErrorKind::Invalide7BitValue( byte ).into() )
            }
            if ( last==b'\r' ) != (byte == b'\n') {
                return Err( ErrorKind::Invalide7BitSeq( byte ).into() )
            }
            last = byte;
        }
    }
    Ok( TransferEncodedFileBuffer::buffer_is_encoded( buffer, TransferEncoding::_7Bit ) )
}

fn encode_8bit( buffer: FileBuffer ) -> Result<TransferEncodedFileBuffer> {
    {
        let data: &[u8] = &*buffer;

        let mut last = b'\0';
        for byte in data.iter().cloned() {
            if  byte == 0 {
                bail!( ErrorKind::Invalide8BitValue( byte ) );
            }
            if ( last==b'\r' ) != (byte == b'\n') {
                bail!( ErrorKind::Invalide8BitSeq( byte ) );
            }
            last = byte;
        }
    }
    Ok( TransferEncodedFileBuffer::buffer_is_encoded( buffer, TransferEncoding::_8Bit ) )
}

/// to quote RFC 2045:
/// """[at time of writing] there are no standardized Internet mail transports
///    for which it is legitimate to include
///    unencoded binary data in mail bodies. [...]"""
///
/// nevertheless there is at last one SMTP extension which allows this
/// (chunked),but this library does not support it for now
fn encode_binary( buffer: FileBuffer ) -> Result<TransferEncodedFileBuffer> {
    Ok( TransferEncodedFileBuffer::buffer_is_encoded( buffer, TransferEncoding::Binary ) )
}

fn encode_quoted_printable( buffer: FileBuffer ) -> Result<TransferEncodedFileBuffer> {
    Ok( TransferEncodedFileBuffer::buffer_is_encoded(
        buffer.with_data( |data| quoted_printable::normal_encode( data ).into() ),
        TransferEncoding::QuotedPrintable
    ) )
}

fn encode_base64( buffer: FileBuffer ) -> Result<TransferEncodedFileBuffer> {
    Ok( TransferEncodedFileBuffer::buffer_is_encoded(
        buffer.with_data( |data| base64::normal_encode(data).into_bytes() ),
        TransferEncoding::Base64
    ) )
}
