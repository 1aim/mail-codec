use std::ops::Deref;


use super::file_meta::FileMeta;
use mime::{AnyMediaType, TEXT, CHARSET};

// WHEN_FEATURE(more_charsets)
// for now this is just a vector,
// but when <encodings> is used to support
// non-utf8/non-ascii encodings this will
// have more fields, like e.g. `encoding: EncodingSpec`
#[derive(Debug, Clone)]
pub struct FileBuffer {
    content_type: AnyMediaType,
    data: Vec<u8>,
    file_meta: FileMeta
}


impl FileBuffer {

    pub fn new( content_type: AnyMediaType, data: Vec<u8> ) -> FileBuffer {
        FileBuffer::new_with_file_meta( content_type, data, Default::default() )
    }

    pub fn new_with_file_meta( content_type: AnyMediaType, data: Vec<u8>, file_meta: FileMeta ) -> FileBuffer {
        FileBuffer { content_type, data, file_meta }
    }

    pub fn with_data<FN>( mut self, modif: FN ) -> Self
        where FN: FnOnce( Vec<u8> ) -> Vec<u8>
    {
        self.data = modif( self.data );
        self
    }

    pub fn content_type( &self ) -> &AnyMediaType {
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

