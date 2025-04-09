  $ enable amend

  $ newclientrepo
  $ setconfig committemplate.commit-message-fields='Summary,"Test Plan",Reviewers'
  $ cat > msg <<EOS
  > old title
  > 
  > Summary:
  > 
  > old summary
  > 
  > Reviewers: someperson
  > EOS

Can override fields even for initial commit
  $ sl commit -Aql msg --message-field=Reviewers=otherperson
  $ sl log -T '{desc}\n' -r .
  old title
  
  Summary:
  
  old summary
  
  Reviewers: otherperson

Works with metaedit
  $ HGEDITOR=doesnt-exist sl metaedit -r . --message-field="Test Plan=
  > 
  > my test plan
  > "
  $ sl log -T '{desc}\n' -r .
  old title
  
  Summary:
  
  old summary
  
  Test Plan:
  
  my test plan
  
  Reviewers: otherperson

Also works with amend
  $ sl amend -q --message-field="Title=new title
  > " --message-field="Summary=new summary
  > "
  $ sl log -T '{desc}\n' -r .
  new title
  
  Summary: new summary
  
  Test Plan:
  
  my test plan
  
  Reviewers: otherperson

Test error cases
  $ sl metaedit -r . --message-field=Oopsie=Daisy
  abort: field name 'Oopsie' not configured in committemplate.commit-message-fields
  [255]
  $ sl metaedit -r . --message-field=Bad-Format
  abort: --message-field format is name=value or -name to remove
  [255]
  $ sl metaedit -r . --message-field=
  abort: --message-field format is name=value or -name to remove
  [255]

Can remove fields
  $ sl amend -q --message-field="-Test Plan"
  $ sl log -T '{desc}\n' -r .
  new title
  
  Summary: new summary
  
  Reviewers: otherperson
