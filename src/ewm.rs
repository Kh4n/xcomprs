use x11rb::protocol::xproto::Atom;

macro_rules! back_to_enum {
    ($(#[$meta:meta])* $vis:vis enum $name:ident {
        $($(#[$vmeta:meta])* $vname:ident $(= $val:expr)?,)*
    }) => {
        $(#[$meta])*
        $vis enum $name {
            $($(#[$vmeta])* $vname $(= $val)?,)*
        }

        impl std::convert::TryFrom<Atom> for $name {
            type Error = ();

            fn try_from(v: Atom) -> Result<Self, Self::Error> {
                match v {
                    $(x if x == $name::$vname as Atom => Ok($name::$vname),)*
                    _ => Err(()),
                }
            }
        }
    }
}

back_to_enum! {
    pub enum RootWindowHintCodes {
        _NET_CLIENT_LIST_STACKING = 246,
        _NET_ACTIVE_WINDOW = 252,
    }
}
