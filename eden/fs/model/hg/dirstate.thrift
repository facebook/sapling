namespace cpp2 facebook.eden.dirstate

typedef string RelativePath

enum HgUserStatusDirectiveValue {
  Add = 0x0,
  Remove = 0x1,
}

struct DirstateData {
  1: map<RelativePath, HgUserStatusDirectiveValue> directives
}
