use ascii::AsciiChar;

use error::*;
use grammar::is_vchar;
use grammar::encoded_word::EncodedWordContext;
use codec::{ MailEncoder, MailEncodable };
use data::{ Encoding, EncodedWord };

use super::utils::text_partition::{partition, Partition};
use data::Input;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Unstructured {
    //FEATUR_TODO(non_utf8_input): split into parts each possibke having their own encoding
    text: Input,
}

impl Unstructured {
    pub fn from_input( text: Input ) -> Self {
        Unstructured { text }
    }

    pub fn from_string<I>( string: I ) -> Self
        where I: Into<String>
    {
        let string: String = string.into();

        Unstructured {
            text: Input::from( string )
        }
    }

}

impl MailEncodable for Unstructured {
    fn encode<E>( &self, encoder:  &mut E ) -> Result<()> where E: MailEncoder {
        let text: &str = &*self.text;
        if text.len() == 0 {
            return Ok( () )
        }

        let blocks = partition( text )?;

        //UNWRAP_SAFETY: is safe because we pushed at last one (current_block)
        for block in blocks.into_iter() {
            match block {
                Partition::VCHAR( data ) => {
                    let needs_encoding = data
                        .chars()
                        .any(|ch| !is_vchar( ch, encoder.mail_type() ) );

                    if needs_encoding {
                        EncodedWord::write_into( encoder,
                                                 data,
                                                 Encoding::QuotedPrintable,
                                                 EncodedWordContext::Text );
                    } else {
                        // if needs_encoding is false all chars a vchars wrt. the mail
                        // type, therefore if the mail type is Ascii this can only be
                        // Ascii. Note that even writing Utf8 to a Ascii mail is safe
                        // wrt. rust, but incorrect nevertheless.
                        encoder.write_str_unchecked( data )
                    }
                },
                Partition::SPACE( data ) => {
                    //NOTE: the usage of write_fws is relevant for braking the line and CRLF
                    // is still semantically ignored BUT, ther cant be any comments here,
                    // as we are in a unstructured header field
                    let mut had_fws = false;
                    for char in data.chars() {
                        if char == '\r' || char == '\n' {
                            continue;
                        } else if had_fws {
                            //OPTIMIZE: from_unchecked as char is always a char in this context
                            encoder.write_char( AsciiChar::from( char ).unwrap() );
                        } else {
                            //FIXME allow writing fws based on '\t'
                            encoder.write_fws();
                            had_fws = true;
                        }
                    }
                    if !had_fws {
                        //currently this can only happen if data only consists of '\r','\n'
                        //NOTE: space has to be at last one horizontal-white-space
                        // (required by the possibility of VCHAR partitions beeing
                        //  encoded words)
                        encoder.write_fws();
                    }
                }
            }

        }

        Ok( () )
    }
}
