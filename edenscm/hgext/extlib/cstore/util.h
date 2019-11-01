/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

template <class T>
void removeFromVector(std::vector<T>& vec, T& item) {
  for (auto it = vec.begin(); it != vec.end(); ++it) {
    if (*it == item) {
      vec.erase(it);
      break;
    }
  }
}
