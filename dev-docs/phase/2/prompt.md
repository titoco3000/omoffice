Lets add a simple JWT auth system.
For now, it will be very simple:

A rotating JWT_KEY AWS secrets, rotating every sunday

A /auth/request-key/{email} route that returns the generated key. Only accepts emails from a vec! whitelist.
JWT payload includes email and an expiry date, 48h to the future.

A easy way of setting routes as 'protected' ex "set all routes /private/* as protected"

Protected routes need the bearer key.
Routes called with invalid/expired/missing keys return the appropriate errors