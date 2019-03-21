// Copyright Facebook, Inc. 2019

#[cfg(feature = "buildinfo")]
#[link(name = "buildinfo", kind = "static")]
extern "C" {
    pub fn print_buildinfo();
}
