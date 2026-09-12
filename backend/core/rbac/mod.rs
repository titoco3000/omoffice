/*
Import db, org_invites, web_messenger

Allows full role-based-access-control administration: 
- org creation/deletion
- user org/global renaming
- role asignment
- User invitation (with roles)

admin may only leave org if there is other admin in the org already.

Subscribes to the user connection event of web_messenger to track if user is online
*/ 

