  $ enable smartlog

Rejects invalid size-factors:

  $ sl init --config format.use-virtual-repo-with-size-factor=-1 virtual-1
  abort: format.use-virtual-repo-with-size-factor must be between 0 and 34
  [255]
  $ sl init --config format.use-virtual-repo-with-size-factor=35 virtual35
  abort: format.use-virtual-repo-with-size-factor must be between 0 and 34
  [255]

Virtual repo with size-factor=2:

  $ sl init --config format.use-virtual-repo-with-size-factor=2 virtual2
  $ cd virtual2

Smartlog works:

  $ sl
  o  commit:      c20cdc010000
  │  bookmark:    virtual/main
  ~  user:        test <test@example.com>
     date:        Sat Oct 25 09:35:59 2025 +0000
     summary:     synthetic commit 121869

Total file count and size is reasonable (~80MB):

  $ sl go 'roots(all())' -q
  $ FILES=$TESTTMP/files
  $ sl files > $FILES

  >>> import os, stat
  >>> def check_count_file_size():
  ...     """Check $FILES is sane (no dups, no dirs). Return count and size."""
  ...     with open(getenv('FILES')) as f:
  ...         paths = [p.strip() for p in f]
  ...     assert len(paths) == len(set(paths)), 'paths should be unique'
  ...     total_size = 0
  ...     for path in paths:
  ...         st = os.lstat(path)
  ...         assert not stat.S_ISDIR(st.st_mode), f'{path} should not be a dir'
  ...         # skip non-regular files (e.g. symlinks) that have different sizes on Windows
  ...         if stat.S_ISREG(st.st_mode):
  ...             total_size += st.st_size
  ...     return len(paths), total_size

  >>> check_count_file_size()
  (6084, 74922986)

Checkout virtual/main with more files (~800MB):

  $ sl go 'virtual/main' -q
  $ sl files > $FILES

  >>> check_count_file_size()
  (57364, 737830235)

Virtual repo with size-factor=0 works too:

  $ cd
  $ sl init --config format.use-virtual-repo-with-size-factor=0 virtual0
  $ cd virtual0
  $ sl status --change 'virtual/main'
  M V/red-b/e/IV-IV/j/I-II
  M V/red-b/e/IV-IV/j/V
  M V/red-b/e/VIII/cherry
  M V/red-b/e/VIII/grape-II
  M V/red-b/e/VIII/lemon-IV/II
  M V/red-b/e/VIII/lemon-IV/VI
  M V/red-b/e/VIII/pear-II

Sample check a file content:

  $ sl cat -r virtual/main V/red-b/e/IV-IV/j/III-II
  Rabbit, and had just begun to dream that she was walking by the pope, was
  soon submitted to by all three to settle the question, and they drew all
  manner of things-everything that begins with an M, such as mouse-traps, and
  the blades of grass, but she could not remember the simple rules their
  friends had taught them: such as, Sure, I don't see how he can thoroughly
  enjoy The pepper when he finds out who I _was_ when I breathe'!
  
  Don't let me hear the name again!
  
  You grant that?
  
  Pigeon, but in a fight with another hedgehog, which seemed to be no sort of
  lullaby to it in her life; it was a bright idea came into her face.
  
  He moved on as he spoke, we were little, the Mock Turtle yet?
  
  Bill's got to the Knave Turn them over!
  
  Alice desperately: he's perfectly idiotic! And she tried hard to whistle to
  it; but she could for sneezing.
  
  However, everything is queer to-day.
  
  I got up in spite of all the creatures wouldn't be so easily offended!
  
  We quarrelled last March-just before _he_ went mad, you know- (pointing
  with his head! or Off with her arms round it as well as she went hunting
  about, and crept a little scream of laughter. Oh, hush! the Rabbit angrily.
  Here! Come and help me out of its mouth, and addressed her in the sea,
  though you mayn't believe it-
  
  D, she added in a hoarse growl, the world go round!'
  
  I quite forgot how to speak again. In a minute or two, looking for it, you
  know- D, she added aloud.
  
  HEARTS.
  
  Alice was beginning to feel very sleepy and stupid), whether the blows hurt
  it or not.
  
  CHORUS.
  
  Wow! wow! wow!
  
  The Queen's argument was, that anything that looked like the Queen? said
  the Hatter, and he went on in the sea, and in another moment down went Alice
  after it, never once considering how in the house till she was now only ten
  inches high, and her face like the name: however, it may kiss my hand if it
  makes me grow larger, I can find out the answer to it? said the Mock Turtle
  said: no wise fish would go anywhere without a porpoise.
  
  Why, said the Pigeon. I can reach the key; and if I had to stoop to save
  her neck kept getting entangled among the distant green leaves.
  
  She felt very glad to get hold of its mouth and yawned once or twice, and
  shook itself. Then it wasn't very civil of you to leave the room, when her
  eye fell upon a little scream of laughter. Oh, hush! the Rabbit whispered in
  a tone of great surprise.
  
  I then? Tell me that first, and then, if I shall sit here, the Footman
  remarked, till tomorrow-
  
  Do as I mentioned before, And have grown most uncommonly fat; Yet you
  turned a corner, Oh my ears and whiskers, how late it's getting! She was
  walking by the time it all seemed quite natural); but when you have to fly;
  and the others looked round also, and all the while, and fighting for the
  hot day made her next remark. Then the eleventh day must have been changed
  several times since then.
  
  I sleep when I got up this morning? I almost think I may as well be at
  school at once. And in she went.
  
  Then you shouldn't talk, said the Gryphon.
  
  Caterpillar called after it; and while she was up to the Duchess: and the
  Queen said-
  
  Caucus-race.
  
  What _is_ the use of this was the _best_ butter, you know.
  
  Then you shouldn't have put it in asking riddles that have no sort of thing
  never happened, and now for the baby, it was only a mouse that had slipped
  in like herself.
  
  Whoever lives there, thought Alice, they're sure to make _some_ change in
  my size; and as the large birds complained that they _would_ put their heads
  downward! The Antipathies, I think- (for, you see, as she remembered that
  she ran across the field after it, Mouse dear! Do come back with the clock.
  For instance, if you only walk long enough.
  
  Beautiful, beauti-FUL SOUP!
  
  Alice; that's not at all a pity. I said pig, replied Alice; and Alice
  looked all round her at the time it all seemed quite natural to Alice
  severely. What are you getting on now, my dear? it continued, turning to
  Alice a little while, however, she waited for some time after the birds!
  Why, she'll eat a bat? when suddenly, thump! thump! down she came in sight
  of the house, Let us both go to on the door with his head! she said, than
  waste it in a melancholy tone: it doesn't mind.
  
  Said his father; don't give yourself airs! Do you know what it' means well
  enough, when _I_ find a number of cucumber-frames there must be! thought
  Alice. I'm a-I'm a-
  
  Alice again, for really I'm quite tired and out of a sea of green leaves
  that lay far below her.
  
  Well, it must be shutting up like telescopes: this time she had never had
  to kneel down on the top of it.
  
  A barrowful of _what?_ The other side of the words a little, From the
  Queen. Can you play croquet with the Lory, who at last she stretched her
  arms round it as you liked.
  
  I'm Mabel, I'll stay down here till I'm somebody else'-but, oh dear! cried
  Alice, with a cart-horse, and expecting every moment to be trampled under
  its feet, I wonder what they _will_ do next! If they had been to the door,
  she walked up towards it rather timidly, saying to herself That's quite
  enough-I hope I shan't grow any more-As it is, said the Duchess. An
  invitation for the baby, it was sneezing and howling alternately without a
  moment's delay would cost them their lives.
  
  Your hair wants cutting, said the Mock Turtle to the Caterpillar, and the
  baby-the fire-irons came first; then followed a shower of little pebbles
  came rattling in at all? said Alice, I've often seen a good opportunity for
  showing off her knowledge, as there seemed to be treated with respect.
  
  Alice cautiously replied: but I _think_ I can remember feeling a little
  feeble, squeaking voice, (That's Bill, thought Alice,) Well, I never went to
  the Mock Turtle said: advance twice, set to work throwing everything within
  her reach at the door- Pray, what is the capital of Paris, and Paris is the
  reason and all the children she knew that were of the window, and some of
  the evening, beautiful Soup! Beau-ootiful Soo-oop! Beau-ootiful Soo-oop!
  Beau-ootiful Soo-oop! Soo-oop of the garden, and marked, with one eye, How
  the Owl and the moment she felt a little shriek, and went on eagerly: There
  is another shore, you know, said Alice sharply, for she had got so close to
  her to begin. For, you see, as she spoke. I must go by the little golden key
  in the pool as it went.
  
  Gryphon at the thought that she was talking. How _can_ I have dropped them,
  I wonder? As she said to herself as she spoke; either you like: they're both
  mad. France- Then turn not pale, beloved snail, but come and join the dance?
  Will you, won't you, will you join the dance? Will you, won't you, will you,
  won't you, will you join the dance? Will you, won't you join the dance?
  
  And yesterday things went on planning to herself in a very decided tone:
  tell her something worth hearing. For some minutes it seemed quite natural
  to Alice severely. What are you getting on? said Alice, swallowing down her
  flamingo, and began singing in its sleep _Twinkle, twinkle, twinkle,
  twinkle_- and went back to the game, the Queen merely remarking that a
  red-hot poker will burn you if you drink much from a bottle marked poison,
  it is all the right size for going through the air! Do you think you might
  like to go on crying in this way! Stop this moment, I tell you! said Alice.
  What sort of a feather flock together.'
  
  I try the patience of an oyster!
  
  Dormouse, without considering at all anxious to have no sort of circle,
  (the exact shape doesn't matter, it said,) and then dipped suddenly down, so
  suddenly that Alice quite jumped; but she did so, and giving it a violent
  blow underneath her chin: it had come back and see what was on the second
  thing is to do that, said the Cat.
  
  It belongs to the conclusion that it would be quite as safe to stay in here
  any longer!
  
  You insult me by talking such nonsense!
  
  But they _have_ their tails in their mouths-and they're all over with
  diamonds, and walked off; the Dormouse again, so she set to work shaking him
  and punching him in the pictures of him), while the rest waited in silence.
  At last the Gryphon at the Footman's head: it just at present-at least I
  know is, something comes at me like that! He got behind Alice as it was not
  an encouraging tone.
  
  Suppose we change the subject.
  
  Not I! said the Gryphon. I mean, what makes them bitter-and-and
  barley-sugar and such things that make children sweet-tempered. I only wish
  it was, and, as she had never seen such a nice little histories about
  children who had got its head impatiently, and said, So you did, said the
  Mouse, sharply and very soon had to kneel down on her toes when they liked,
  so that by the pope, was soon submitted to by the fire, stirring a large
  pool all round the neck of the house down! said the Caterpillar sternly.
  Explain yourself!
  
  Gryphon: I went to school in the pool a little bottle that stood near the
  house till she got used to come once a week: _he_ taught us Drawling,
  Stretching, and Fainting in Coils.
  
  Alice asked in a hurried nervous manner, smiling at everything that Alice
  quite jumped; but she got up, and there she saw maps and pictures hung upon
  pegs. She took down a large crowd collected round it: there were three
  gardeners at it, and talking over its head. Very uncomfortable for the fan
  and gloves-that is, if I know all sorts of things, and she, oh! she knows
  such a new kind of serpent, that's all I can tell you his history.
  
  Soup_,' will you, won't you, won't you, will you, won't you, will you,
  won't you, will you, won't you, will you join the dance. Will you, won't you
  join the dance?
  
  The hedgehog was engaged in a whisper.)
  
  Talking of axes, said the Gryphon in an offended tone, Hm! No accounting
  for tastes! Sing her _Turtle Soup_,' will you, won't you join the dance?
  
  Alice, Have you seen the Mock Turtle.
  
  Talking of axes, said the Cat; and this was not much surprised at this, but
  at the mushroom (she had kept a piece of rudeness was more hopeless than
  ever: she sat down again very sadly and quietly, and looked at the bottom of
  a tree.
  
  Ah, well! It means much the same height as herself; and when she got up,
  and began to get her head impatiently; and, turning to Alice.
  
  That _was_ a most extraordinary noise going on within-a constant howling
  and sneezing, and every now and then, thought she, if people had all to lie
  down on her face in some alarm. This time there were three little sisters,
  the Dormouse shook its head impatiently, and said, very gravely, I think,
  you ought to eat the comfits: this caused some noise and confusion, as the
  door between us. For instance, suppose it doesn't seem to come down the
  chimney, has he? said Alice thoughtfully: but then-I shouldn't be hungry for
  it, you know.
  
  The soldiers were silent, and looked along the course, here and there was a
  dispute going on shrinking rapidly: she soon made out the Fish-Footman was
  gone, and, by the soldiers, who of course you know the meaning of half those
  long words, and, what's more, I don't know what it' means well enough, when
  _I_ find a number of bathing machines in the kitchen that did not at all a
  pity. I said pig, replied Alice; and Alice could hardly hear the Rabbit just
  under the circumstances. There was a large rabbit-hole under the table: she
  opened it, and found that, as nearly as large as himself, and this time the
  Mouse was swimming away from him, and said No, never) -so you can find it.
  And yet you incessantly stand on your head- Do you know that cats _could_
  grin. Turtle angrily: really you are painting those roses?
  
  That's very curious.
  
  If I don't like the Mock Turtle.
  
  Gryphon replied rather crossly: of course had to run back into the open
  air. If I eat one of its mouth, and addressed her in an encouraging tone.
  
  Alice, she went on again:-
  
  Alice, every now and then I'll tell him-it was for bringing the cook took
  the opportunity of showing off a bit hurt, and she looked at the March Hare
  and the baby with some curiosity. What a curious feeling! said Alice; but
  when you have to fly; and the reason of that?
  
  By this time it vanished quite slowly, beginning with the bread-knife.
  
  And have grown most uncommonly fat; Yet you balanced an eel on the breeze
  that followed them, the Mock Turtle: crumbs would all wash off in the wood,
  continued the Gryphon.
  
  Oh, I beg your pardon! cried Alice hastily, afraid that it would make with
  the game, the Queen was to find herself still in existence; and now here I
  am older than you, and don't speak a word till I've finished.
  
  It's enough to try the effect: the next verse, the Gryphon as if he had a
  wink of sleep these three weeks!
  
  Cat; and this time it vanished quite slowly, beginning with the Queen, and
  take this young lady to see it trot away quietly into the loveliest garden
  you ever see you again, you dear old thing! said Alice, and she soon made
  out the proper way of expecting nothing but the Rabbit came up to Alice, and
  tried to get an opportunity of saying to herself Now I can listen all day to
  such stuff? Be off, or I'll kick you down stairs!
  
  I'll just see what would be a very little way off, and she did not like the
  three gardeners instantly jumped up, and there stood the Queen in front of
  the Nile On every golden scale!
  
  Dormouse had closed its eyes by this time, and was looking at Alice the
  moment she felt certain it must be getting somewhere near the house till she
  shook the house, and found in it about four feet high. Whoever lives there,
  thought Alice, shall I _never_ get any older than you, and don't look at the
  Mouse's tail; but why do you know why it's called a whiting?
  
  Was kindly permitted to pocket the spoon: While the Duchess sneezed
  occasionally; and as it was neither more nor less than no time she'd have
  everybody executed, all round. (It was this last remark.
  
  Gryphon, lying fast asleep in the distance, and she felt sure it would make
  with the end of his tail. As if it wasn't very civil of you to death.'
  
  Alice, thinking it was quite impossible to say but It belongs to a mouse,
  you know. But do cats eat bats? and sometimes, Do bats eat cats? for, you
  see, Miss, this here ought to have no answers.
  
  Dormouse again, so violently, that she looked down at her as hard as it
  could go, and making quite a large fan in the air. She did not like the look
  of the creature, but on the other end of half those long words, and, what's
  more, I don't remember where.
  
  I was, I shouldn't want _yours_: I don't know of any use, now, thought poor
  Alice, that she began again. I dare say you never even introduced to a
  snail. There's a porpoise close behind her, listening: so she went on, you
  throw them, and just as I was a bright brass plate with the birds and
  animals that had fallen into it: there were three gardeners at it, and
  burning with curiosity, she ran off as hard as she was exactly the right
  house, because the chimneys were shaped like the look of the song, perhaps?
  
  His voice has a timid voice at her for a good opportunity for showing off
  her head!
  
  Hatter, it woke up again with a knife, it usually bleeds; and she grew no
  larger: still it had made.
  
  For he can thoroughly enjoy The pepper when he pleases!
  
  Coils.
  
  What was that? inquired Alice.
  
  Those whom she sentenced were taken into custody by the whole thing, and
  longed to change the subject.
  
  Hardly knowing what she did, she picked up a little hot tea upon its
  forehead (the position in which the March Hare moved into the garden at
  once; but, alas for poor Alice! when she went out, but it makes rather a
  handsome pig, I think. And she began thinking over all she could do, lying
  down with one finger; and the White Rabbit was still in sight, and no more
  of the wood-(she considered him to be two people. But it's no use in saying
  anything more till the eyes appeared, and then raised himself upon tiptoe,
  put his mouth close to them, and it'll sit up and said, very gravely, I
  think, you ought to be a queer thing, to be afraid of it.
  
  However, this bottle was a different person then.
  
  Queen's voice in the house, and the words all coming different, and then
  she heard one of the e-e-evening, Beautiful, beautiful Soup!
  
  Alice, who felt ready to agree to everything that Alice had got its head
  down, and the Hatter grumbled: you shouldn't have put it more clearly, Alice
  replied in a great hurry.
  
  Dormouse shook itself, and began singing in its hurry to change them- when
  she was small enough to drive one crazy!
  
  Edwin and Morcar, the earls of Mercia and Northumbria-'
  
  Alice (she was so much already, that it might belong to one of the
  e-e-evening, Beautiful, beautiful Soup!
  
  I'll be judge, I'll be judge, I'll be jury,' Said cunning old Fury: I'll
  try and repeat '_Tis the voice of the conversation. Alice felt so desperate
  that she began fancying the sort of chance of her favourite word moral,' and
  the baby was howling so much at first, the two creatures, who had not got
  into a line along the passage into the air off all its feet at the Gryphon
  in an angry tone, Why, Mary Ann, and be turned out of this remark, and
  thought it had some kind of thing never happened, and now here I am to see
  if there were _two_ little shrieks, and more puzzled, but she ran with all
  speed back to my jaw, Has lasted the rest of it had _very_ long claws and a
  fall, and a Canary called out The race is over! and they repeated their
  arguments to her, still it was good manners for her neck kept getting
  entangled among the trees, a little faster? said a whiting to a mouse: she
  had drunk half the bottle, she found it advisable-'
  
  Alice,) Well, I hardly know-No mor (no-eol)

Maximum factor_size:

  $ cd
  $ sl init --config format.use-virtual-repo-with-size-factor=34 virtual34
  $ cd virtual34
  $ sl
  o  commit:      e2000000000c
  │  bookmark:    virtual/main
  ~  user:        test <test@example.com>
     date:        Wed Dec 29 08:00:00 9999 +0000
     summary:     synthetic commit 523419074428929
